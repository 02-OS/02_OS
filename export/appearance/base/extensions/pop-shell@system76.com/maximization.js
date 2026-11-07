import Meta from 'gi://Meta';
import { PACKAGE_VERSION } from 'resource:///org/gnome/shell/misc/config.js';

// Mutter 49 removed the directions argument. Check the Shell version rather
// than the enum's presence: newer Mutter releases also expose MaximizeFlags
// for separate flag setters.
const usesDirections = Number.parseInt(PACKAGE_VERSION, 10) < 49;

export function maximize(window) {
    if (usesDirections)
        window.maximize(Meta.MaximizeFlags.BOTH);
    else
        window.maximize();
}

export function unmaximize(window) {
    if (usesDirections)
        window.unmaximize(Meta.MaximizeFlags.BOTH);
    else
        window.unmaximize();
}
