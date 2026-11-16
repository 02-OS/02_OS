import * as log from './log.js';
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
const { byteArray } = imports;
export class LauncherService {
    constructor(service, callback, onClosed = null) {
        this.service = service;
        this.onClosed = onClosed;
        const generator = (stdout, res) => {
            try {
                const [bytes] = stdout.read_line_finish(res);
                if (bytes) {
                    const string = byteArray.toString(bytes);
                    callback(JSON.parse(string));
                    this.service.stdout.read_line_async(0, this.service.cancellable, generator);
                }
                else if (this.onClosed) {
                    this.onClosed();
                }
            }
            catch (why) {
                if (why.matches(Gio.IOErrorEnum, Gio.IOErrorEnum.CANCELLED)) {
                    return;
                }
                log.error(`failed to read response from launcher service: ${why}`);
                if (this.onClosed)
                    this.onClosed();
            }
        };
        this.service.stdout.read_line_async(0, this.service.cancellable, generator);
    }
    activate(id) {
        this.send({ Activate: id });
    }
    activate_context(id, context) {
        this.send({ ActivateContext: { id, context } });
    }
    complete(id) {
        this.send({ Complete: id });
    }
    context(id) {
        this.send({ Context: id });
    }
    exit() {
        this.send('Exit');
        this.service.cancellable.cancel();
        const service = this.service;
        GLib.timeout_add(GLib.PRIORITY_DEFAULT, 100, () => {
            if (service.stdout.has_pending() || service.stdin.has_pending())
                return true;
            const close_stream = (stream) => {
                try {
                    stream.close(null);
                }
                catch (why) {
                    log.error(`failed to close pop-launcher stream: ${why}`);
                }
            };
            close_stream(service.stdin);
            close_stream(service.stdout);
            return false;
        });
    }
    query(search) {
        this.send({ Search: search });
    }
    quit(id) {
        this.send({ Quit: id });
    }
    select(id) {
        this.send({ Select: id });
    }
    send(object) {
        const message = JSON.stringify(object);
        try {
            this.service.stdin.write_all(message + '\n', null);
        }
        catch (why) {
            log.error(`failed to send request to pop-launcher: ${why}`);
            if (this.onClosed)
                this.onClosed();
        }
    }
}
