import Adw from 'gi://Adw';
import Gio from 'gi://Gio';
import Gtk from 'gi://Gtk';
import {ExtensionPreferences} from 'resource:///org/gnome/Shell/Extensions/js/extensions/prefs.js';

export default class AgentStatusPreferences extends ExtensionPreferences {
    fillPreferencesWindow(window) {
        const settings = this.getSettings();
        const page = new Adw.PreferencesPage({
            title: '02 Agent',
            icon_name: 'dialog-information-symbolic',
        });
        const group = new Adw.PreferencesGroup({
            title: 'Status',
            description: 'Reads the local 02-agent status file. Nothing is sent off this machine.',
        });

        const spin = new Gtk.SpinButton({
            adjustment: new Gtk.Adjustment({
                lower: 5,
                upper: 120,
                step_increment: 1,
                page_increment: 5,
            }),
            valign: Gtk.Align.CENTER,
        });
        settings.bind('poll-seconds', spin, 'value', Gio.SettingsBindFlags.DEFAULT);
        const interval = new Adw.ActionRow({ title: 'Poll interval (seconds)' });
        interval.add_suffix(spin);

        const notifications = new Adw.SwitchRow({ title: 'Notifications' });
        settings.bind('notifications', notifications, 'active', Gio.SettingsBindFlags.DEFAULT);

        group.add(interval);
        group.add(notifications);
        page.add(group);
        window.add(page);
    }
}
