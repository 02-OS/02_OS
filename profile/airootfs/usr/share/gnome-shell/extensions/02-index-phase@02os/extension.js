import GLib from 'gi://GLib';
import Gio from 'gi://Gio';
import St from 'gi://St';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import * as PanelMenu from 'resource:///org/gnome/shell/ui/panelMenu.js';
import * as PopupMenu from 'resource:///org/gnome/shell/ui/popupMenu.js';
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';

const PHASES = new Set(['idle', 'indexing', 'complete', 'stale']);

export function phaseFromText(text) {
    const line = `${text ?? ''}`.split('\n')[0].trim();
    return PHASES.has(line) ? line : 'idle';
}

function phasePath() {
    const data = GLib.getenv('XDG_DATA_HOME')
        || GLib.build_filenamev([GLib.get_home_dir(), '.local', 'share']);
    return GLib.build_filenamev([data, '02-agent', 'index-phase']);
}

export default class IndexPhaseExtension extends Extension {
    enable() {
        this._cancellable = new Gio.Cancellable();
        this._lastUpdated = 'never';
        this._currentPhase = 'idle';
        this._button = new PanelMenu.Button(0.0, '02 index phase', false);
        this._label = new St.Label({
            text: '02 index: idle',
            style_class: '02-index-phase',
        });
        this._button.add_child(this._label);

        this._timestampItem = new PopupMenu.PopupMenuItem('Last indexed: never', {
            reactive: false,
        });
        this._button.menu.addMenuItem(this._timestampItem);
        this._button.menu.addAction('Refresh index status', () => this._refresh());
        this._button.menu.addAction('Open 02 dashboard', () => this._openDashboard());
        this._button.menu.addAction('Copy index diagnostics', () => this._copyDiagnostics());

        this._monitor = null;
        try {
            const file = Gio.File.new_for_path(phasePath());
            if (typeof file.monitor_file === 'function') {
                this._monitor = file.monitor_file(Gio.FileMonitorFlags.NONE, this._cancellable);
                this._monitorId = this._monitor.connect('changed', () => this._refresh());
            }
        } catch (_error) {
            this._monitor = null;
        }

        Main.panel.addToStatusArea('02-index-phase', this._button);
        this._refresh();
        this._timeout = GLib.timeout_add_seconds(GLib.PRIORITY_DEFAULT, 15, () => {
            this._refresh();
            return GLib.SOURCE_CONTINUE;
        });
    }

    disable() {
        if (this._timeout) {
            GLib.source_remove(this._timeout);
            this._timeout = 0;
        }
        if (this._cancellable) {
            this._cancellable.cancel();
            this._cancellable = null;
        }
        if (this._monitor) {
            try {
                if (this._monitorId)
                    this._monitor.disconnect(this._monitorId);
                this._monitor.cancel();
            } catch (_error) {}
            this._monitor = null;
            this._monitorId = 0;
        }
        this._timestampItem = null;
        this._button?.destroy();
        this._button = null;
        this._label = null;
    }

    _applyPhaseText(text) {
        if (!this._button)
            return;
        const phase = phaseFromText(text);
        if (phase !== this._currentPhase || this._lastUpdated === 'never') {
            const now = GLib.DateTime.new_now_local();
            this._lastUpdated = now ? now.format('%H:%M:%S') : 'updated';
        }
        this._currentPhase = phase;
        this._label?.set_text(`02 index: ${phase}`);
        if (this._timestampItem?.label)
            this._timestampItem.label.set_text(`Last indexed: ${this._lastUpdated} (${phase})`);
    }

    _refresh() {
        try {
            const file = Gio.File.new_for_path(phasePath());
            if (typeof file.load_contents_async === 'function') {
                file.load_contents_async(this._cancellable, (source, res) => {
                    try {
                        const [ok, bytes] = source.load_contents_finish(res);
                        const text = ok ? new TextDecoder().decode(bytes) : '';
                        this._applyPhaseText(text);
                    } catch (_error) {
                        this._applyPhaseText('');
                    }
                });
                return;
            }
            const [ok, bytes] = file.load_contents(this._cancellable);
            this._applyPhaseText(ok ? new TextDecoder().decode(bytes) : '');
        } catch (_error) {
            this._applyPhaseText('');
        }
    }

    _openDashboard() {
        try {
            Gio.Subprocess.new(['02', 'dashboard'], Gio.SubprocessFlags.NONE);
            Gio.AppInfo.launch_default_for_uri('http://127.0.0.1:8732', null);
        } catch (_error) {
            Main.notify('02 index', 'Could not launch 02 dashboard');
        }
    }

    _copyDiagnostics() {
        try {
            const summary = `02-index phase=${this._currentPhase} updated=${this._lastUpdated}`;
            St.Clipboard.get_default().set_text(St.ClipboardType.CLIPBOARD, summary);
            Main.notify('02 index', 'Copied index diagnostics to clipboard');
        } catch (_error) {
            Main.notify('02 index', 'Could not copy diagnostics');
        }
    }
}
