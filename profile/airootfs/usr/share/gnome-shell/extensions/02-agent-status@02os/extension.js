import Clutter from 'gi://Clutter';
import GLib from 'gi://GLib';
import Gio from 'gi://Gio';
import St from 'gi://St';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import * as PanelMenu from 'resource:///org/gnome/shell/ui/panelMenu.js';
import * as PopupMenu from 'resource:///org/gnome/shell/ui/popupMenu.js';
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';

const EMPTY = { index_phase: 'idle', watched_count: 0, approvals: [] };

export function matchingApproval(approvals, typed) {
    const needle = `${typed ?? ''}`.trim().toLowerCase();
    if (!/^[0-9a-f]{64}$/.test(needle))
        return null;
    return (approvals ?? []).find(row => `${row.args_hash ?? ''}`.toLowerCase() === needle) ?? null;
}

function dataHome() {
    return GLib.getenv('XDG_DATA_HOME')
        || GLib.build_filenamev([GLib.get_home_dir(), '.local', 'share']);
}

function statusPath() {
    return GLib.build_filenamev([dataHome(), '02-agent', 'status.json']);
}

function socketPath() {
    const runtime = GLib.getenv('XDG_RUNTIME_DIR');
    if (runtime)
        return GLib.build_filenamev([runtime, '02agent.sock']);
    return GLib.build_filenamev([dataHome(), '02-agent', '02agent.sock']);
}

function readStatus() {
    try {
        const file = Gio.File.new_for_path(statusPath());
        const [ok, bytes] = file.load_contents(null);
        if (!ok)
            return EMPTY;
        const parsed = JSON.parse(new TextDecoder().decode(bytes));
        return {
            index_phase: `${parsed.index_phase ?? 'idle'}`,
            watched_count: Number(parsed.watched_count) || 0,
            approvals: Array.isArray(parsed.approvals) ? parsed.approvals : [],
        };
    } catch (_error) {
        return EMPTY;
    }
}

function sendDecide(row, decision, typed) {
    const client = new Gio.SocketClient();
    const address = new Gio.UnixSocketAddress({ path: socketPath() });
    const connection = client.connect(address, null);
    const payload = {
        jsonrpc: '2.0',
        id: 1,
        method: 'daemon/decide',
        params: {
            repo_path: row.repo_path,
            id: `gnome-${GLib.get_real_time()}`,
            agt_run_id: row.run_id ?? '',
            agt_command_id: row.command_id ?? '',
            args_hash: row.args_hash,
            confirmed_hash: typed.trim(),
            decision,
            approver: 'desktop-user',
            invoker: 'agent',
        },
    };
    const bytes = new TextEncoder().encode(`${JSON.stringify(payload)}\n`);
    connection.get_output_stream().write_all(bytes, null);
    const input = new Gio.DataInputStream({ base_stream: connection.get_input_stream() });
    const [reply] = input.read_line_utf8(null);
    connection.close(null);
    return reply ? JSON.parse(reply) : null;
}

export default class AgentStatusExtension extends Extension {
    enable() {
        this._status = EMPTY;
        this._seenPhase = null;
        this._seenIds = new Set();
        this._settings = null;
        try {
            this._settings = this.getSettings();
        } catch (_error) {
            this._settings = null;
        }

        this._button = new PanelMenu.Button(0.0, '02 agent status', true);
        this._label = new St.Label({
            text: '02 agent: idle',
            style_class: '02-agent-status',
        });
        this._button.add_child(this._label);

        this._entryItem = new PopupMenu.PopupBaseMenuItem({ reactive: false });
        this._entry = new St.Entry({
            hint_text: 'Type the displayed args hash',
            style_class: '02-agent-hash',
            can_focus: true,
        });
        const box = new St.BoxLayout({ orientation: Clutter.Orientation.VERTICAL });
        box.add_child(this._entry);
        this._entryItem.add_child(box);
        this._button.menu.addMenuItem(this._entryItem);

        this._allow = this._button.menu.addAction('Allow confirmed hash', () => this._decide('allow'));
        this._deny = this._button.menu.addAction('Deny confirmed hash', () => this._decide('deny'));
        this._button.menu.addAction('Stop desktop automation', () => this._stopDesktop());

        Main.panel.addToStatusArea('02-agent-status', this._button);
        this._refresh();
        const seconds = this._settings?.get_int('poll-seconds') || 15;
        this._timeout = GLib.timeout_add_seconds(GLib.PRIORITY_DEFAULT, seconds, () => {
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
        this._entry = null;
        this._status = EMPTY;
        this._seenIds = null;
        this._settings = null;
    }

    _notify(body) {
        if (this._settings && !this._settings.get_boolean('notifications'))
            return;
        Main.notify('02 agent', body);
    }

    _refresh() {
        const next = readStatus();
        const phase = next.index_phase || 'idle';
        const count = next.approvals.length;
        this._label?.set_text(`02 agent: ${phase} · ${next.watched_count} repos · ${count} approvals`);

        if (this._seenPhase !== null && this._seenPhase !== phase)
            this._notify(`Index phase is ${phase}`);
        if (this._seenPhase !== null) {
            for (const row of next.approvals) {
                if (row.id && !this._seenIds.has(row.id))
                    this._notify(`Approval ${row.command_id || row.id} is ${row.decision}`);
            }
        }
        this._seenPhase = phase;
        this._seenIds = new Set(next.approvals.map(row => row.id).filter(Boolean));
        this._status = next;
        this._rebuildMenu(next.approvals);
    }

    _rebuildMenu(approvals) {
        for (const item of this._rows ?? [])
            item.destroy();
        this._rows = [];
        const rows = approvals.length ? approvals : [{ id: '', command_id: 'No stored approvals', args_hash: '', decision: '' }];
        for (const row of rows) {
            const label = row.args_hash
                ? `${row.command_id || row.id} · ${row.args_hash} · ${row.decision}`
                : 'No stored approvals';
            const item = new PopupMenu.PopupMenuItem(label);
            this._button.menu.addMenuItem(item, 0);
            this._rows.push(item);
        }
    }

    _stopDesktop() {
        try {
            const config = GLib.getenv('XDG_CONFIG_HOME')
                || GLib.build_filenamev([GLib.get_home_dir(), '.config']);
            const dir = GLib.build_filenamev([config, '02-agent']);
            GLib.mkdir_with_parents(dir, 0o700);
            const path = GLib.build_filenamev([dir, 'desktop-kill']);
            const file = Gio.File.new_for_path(path);
            file.replace_contents('stop\n', null, false, Gio.FileCreateFlags.PRIVATE, null);
            this._notify('Desktop automation is stopped');
        } catch (_error) {
            this._notify('Could not write the desktop kill switch');
        }
    }

    _decide(decision) {
        const typed = this._entry?.get_text() ?? '';
        const row = matchingApproval(this._status.approvals, typed);
        if (!row) {
            this._notify('Type the displayed args hash before deciding');
            return;
        }
        try {
            const reply = sendDecide(row, decision, typed);
            if (reply?.error)
                this._notify(reply.error.message || 'Decision was rejected');
            else
                this._notify(`Recorded a ${decision} from the desktop`);
        } catch (_error) {
            this._notify('02-agentd is not running');
        }
        this._refresh();
    }
}
