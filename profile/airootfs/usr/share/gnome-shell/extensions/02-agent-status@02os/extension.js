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
    const rows = Array.isArray(approvals) ? approvals : [];
    return rows.find(row => row.decision === 'pending' && `${row.args_hash ?? ''}`.toLowerCase() === needle)
        ?? rows.find(row => `${row.args_hash ?? ''}`.toLowerCase() === needle)
        ?? null;
}

export function pendingApprovalCount(approvals) {
    const rows = Array.isArray(approvals) ? approvals : [];
    return rows.filter(row => row && (row.decision === 'pending' || row.decision === 'ask')).length;
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
    if (runtime && runtime.startsWith('/'))
        return GLib.build_filenamev([runtime, '02agent.sock']);
    return GLib.build_filenamev([dataHome(), '02-agent', '02agent.sock']);
}

function parseStatusBytes(bytes) {
    try {
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

function readStatusAsync(cancellable, callback) {
    try {
        const file = Gio.File.new_for_path(statusPath());
        if (typeof file.load_contents_async === 'function') {
            file.load_contents_async(cancellable, (source, res) => {
                try {
                    const [ok, bytes] = source.load_contents_finish(res);
                    callback(ok ? parseStatusBytes(bytes) : EMPTY);
                } catch (_error) {
                    callback(EMPTY);
                }
            });
            return;
        }
        const [ok, bytes] = file.load_contents(cancellable);
        callback(ok ? parseStatusBytes(bytes) : EMPTY);
    } catch (_error) {
        callback(EMPTY);
    }
}

export function buildDecidePayload(row, decision, typed) {
    const normalizedHash = `${typed ?? ''}`.trim().toLowerCase();
    return {
        jsonrpc: '2.0',
        id: 1,
        method: 'daemon/decide',
        params: {
            repo_path: row.repo_path,
            id: `gnome-${GLib.get_real_time()}`,
            pending_id: row.id ?? '',
            confirmation_nonce: row.confirmation_nonce ?? '',
            agt_run_id: row.run_id ?? '',
            agt_command_id: row.command_id ?? '',
            args_hash: row.args_hash,
            confirmed_hash: normalizedHash,
            decision,
            approver: 'desktop-user',
            invoker: 'agent',
        },
    };
}

function sendDecideAsync(row, decision, typed, cancellable, callback) {
    try {
        const client = new Gio.SocketClient();
        const address = new Gio.UnixSocketAddress({ path: socketPath() });
        const payload = buildDecidePayload(row, decision, typed);
        const bytes = new TextEncoder().encode(`${JSON.stringify(payload)}\n`);

        if (typeof client.connect_async === 'function') {
            client.connect_async(address, cancellable, (src, res) => {
                try {
                    const connection = src.connect_finish(res);
                    connection.get_output_stream().write_all(bytes, cancellable);
                    const input = new Gio.DataInputStream({ base_stream: connection.get_input_stream() });
                    if (typeof input.read_line_async === 'function') {
                        input.read_line_async(GLib.PRIORITY_DEFAULT, cancellable, (inStream, readRes) => {
                            try {
                                const [reply] = inStream.read_line_finish_utf8(readRes);
                                connection.close(null);
                                callback(reply ? JSON.parse(reply) : null, null);
                            } catch (err) {
                                try { connection.close(null); } catch (_e) {}
                                callback(null, err);
                            }
                        });
                        return;
                    }
                    const [reply] = input.read_line_utf8(cancellable);
                    connection.close(null);
                    callback(reply ? JSON.parse(reply) : null, null);
                } catch (err) {
                    callback(null, err);
                }
            });
            return;
        }

        const connection = client.connect(address, cancellable);
        connection.get_output_stream().write_all(bytes, cancellable);
        const input = new Gio.DataInputStream({ base_stream: connection.get_input_stream() });
        const [reply] = input.read_line_utf8(cancellable);
        connection.close(null);
        callback(reply ? JSON.parse(reply) : null, null);
    } catch (err) {
        callback(null, err);
    }
}

export default class AgentStatusExtension extends Extension {
    enable() {
        this._status = EMPTY;
        this._seenPhase = null;
        this._seenIds = new Set();
        this._rows = [];
        this._cancellable = new Gio.Cancellable();
        this._settings = null;
        this._settingsChangedIds = [];
        try {
            this._settings = this.getSettings();
        } catch (_error) {
            this._settings = null;
        }

        this._button = new PanelMenu.Button(0.0, '02 agent status', false);
        this._label = new St.Label({
            text: '02 agent: idle',
            style_class: '02-agent-status',
        });
        this._button.add_child(this._label);

        this._approvalsSection = new PopupMenu.PopupMenuSection();
        this._button.menu.addMenuItem(this._approvalsSection);

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

        if (this._settings && typeof this._settings.connect === 'function') {
            this._settingsChangedIds.push(
                this._settings.connect('changed::poll-seconds', () => this._reschedulePoll()),
                this._settings.connect('changed::notifications', () => this._refresh()),
            );
        }

        Main.panel.addToStatusArea('02-agent-status', this._button);
        this._refresh();
        this._reschedulePoll();
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
        if (this._settings && Array.isArray(this._settingsChangedIds)) {
            for (const handlerId of this._settingsChangedIds) {
                try {
                    this._settings.disconnect(handlerId);
                } catch (_error) {}
            }
        }
        this._settingsChangedIds = [];
        for (const item of this._rows ?? [])
            item.destroy();
        this._rows = [];
        this._approvalsSection = null;
        this._entryItem = null;
        this._allow = null;
        this._deny = null;
        this._button?.destroy();
        this._button = null;
        this._label = null;
        this._entry = null;
        this._status = EMPTY;
        this._seenPhase = null;
        this._seenIds = null;
        this._settings = null;
    }

    _reschedulePoll() {
        if (this._timeout) {
            GLib.source_remove(this._timeout);
            this._timeout = 0;
        }
        const rawSeconds = this._settings?.get_int('poll-seconds') || 15;
        const seconds = Math.max(5, Math.min(120, rawSeconds));
        this._timeout = GLib.timeout_add_seconds(GLib.PRIORITY_DEFAULT, seconds, () => {
            this._refresh();
            return GLib.SOURCE_CONTINUE;
        });
    }

    _notify(body) {
        if (this._settings && !this._settings.get_boolean('notifications'))
            return;
        Main.notify('02 agent', body);
    }

    _refresh() {
        readStatusAsync(this._cancellable, next => {
            if (!this._button)
                return;
            const phase = next.index_phase || 'idle';
            const count = next.approvals.length;
            const pending = pendingApprovalCount(next.approvals);
            const badge = pending > 0 ? ` · [${pending} pending]` : '';
            this._label?.set_text(`02 agent: ${phase} · ${next.watched_count} repos · ${count} approvals${badge}`);

            if (this._seenPhase !== null && this._seenPhase !== phase)
                this._notify(`Index phase is ${phase}`);
            if (this._seenPhase !== null && this._seenIds) {
                for (const row of next.approvals) {
                    if (row.id && !this._seenIds.has(row.id))
                        this._notify(`Approval ${row.command_id || row.id} is ${row.decision}`);
                }
            }
            this._seenPhase = phase;
            this._seenIds = new Set(next.approvals.map(row => row.id).filter(Boolean));
            this._status = next;
            this._rebuildMenu(next.approvals);
        });
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
            if (this._approvalsSection)
                this._approvalsSection.addMenuItem(item);
            else
                this._button.menu.addMenuItem(item);
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
            const bytes = new TextEncoder().encode('stop\n');
            if (typeof file.replace_contents_bytes_async === 'function') {
                file.replace_contents_bytes_async(
                    new GLib.Bytes(bytes),
                    null,
                    false,
                    Gio.FileCreateFlags.PRIVATE,
                    this._cancellable,
                    (src, res) => {
                        try {
                            src.replace_contents_finish(res);
                            this._notify('Desktop automation is stopped');
                        } catch (_error) {
                            this._notify('Could not write the desktop kill switch');
                        }
                    },
                );
                return;
            }
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
        sendDecideAsync(row, decision, typed, this._cancellable, (reply, err) => {
            if (err) {
                this._notify('02-agentd is not running');
                return;
            }
            if (reply?.error)
                this._notify(reply.error.message || 'Decision was rejected');
            else
                this._notify(`Recorded a ${decision} from the desktop`);
            this._refresh();
        });
    }
}
