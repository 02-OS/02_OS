import GLib from 'gi://GLib';
import Gio from 'gi://Gio';
import St from 'gi://St';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import * as PanelMenu from 'resource:///org/gnome/shell/ui/panelMenu.js';
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
        this._button = new PanelMenu.Button(0.0, '02 index phase', true);
        this._label = new St.Label({
            text: '02 index: idle',
            style_class: '02-index-phase',
        });
        this._button.add_child(this._label);
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
        this._button?.destroy();
        this._button = null;
        this._label = null;
    }

    _refresh() {
        let text = '';
        try {
            const file = Gio.File.new_for_path(phasePath());
            const [ok, bytes] = file.load_contents(null);
            if (ok)
                text = new TextDecoder().decode(bytes);
        } catch (_error) {
            text = '';
        }
        this._label?.set_text(`02 index: ${phaseFromText(text)}`);
    }
}
