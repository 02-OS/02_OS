import St from 'gi://St';

export function motionEnabled() {
    const settings = St.Settings.get();
    const reduced = settings.reduced_motion ?? settings.reducedMotion;
    return settings.enable_animations &&
        (!St.ReducedMotion || reduced !== St.ReducedMotion.REDUCE);
}
