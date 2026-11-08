//! When the possum must not make a sound.
//!
//! Nobody wants a screaming possum while the screen is locked, the display
//! is off, a screensaver is running, another user is logged in on the
//! screen, or the computer is going to sleep. This module is the pure state
//! machine that tracks those conditions; each platform (see
//! `platform/`) feeds it from two sources:
//!
//! * **Events**: system notifications, so it reacts the moment the screen
//!   locks or the screensaver starts.
//! * **Probes**: a direct reading of the real system state every second, as
//!   a safety net in case a notification is missed or the app starts while
//!   one of these is already true.

/// Why the possum is keeping quiet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    ScreenLocked,
    Screensaver,
    DisplayAsleep,
    UserSwitchedAway,
    SystemSleeping,
}

impl Reason {
    pub fn describe(self) -> &'static str {
        match self {
            Reason::ScreenLocked => "the screen is locked",
            Reason::Screensaver => "the screensaver is running",
            Reason::DisplayAsleep => "the display is off",
            Reason::UserSwitchedAway => "another user is using the screen",
            Reason::SystemSleeping => "the computer is going to sleep",
        }
    }
}

/// Something the system told us about. (Not every platform reports every
/// one: Windows has no screensaver events, Linux only probes.)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub enum Event {
    ScreenLocked,
    ScreenUnlocked,
    ScreensaverStarted,
    ScreensaverStopped,
    DisplaysSlept,
    DisplaysWoke,
    SessionResigned,
    SessionActivated,
    SystemWillSleep,
    SystemDidWake,
}

/// macOS notifications that macOS broadcasts to every app through the
/// distributed notification center. They aren't in any SDK header, but they have been
/// stable for many macOS versions and are widely relied on.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub const DISTRIBUTED_NOTIFICATIONS: [&str; 4] = [
    "com.apple.screenIsLocked",
    "com.apple.screenIsUnlocked",
    "com.apple.screensaver.didstart",
    "com.apple.screensaver.didstop",
];

/// `NSWorkspace` notifications (their names are their own string values).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub const WORKSPACE_NOTIFICATIONS: [&str; 6] = [
    "NSWorkspaceScreensDidSleepNotification",
    "NSWorkspaceScreensDidWakeNotification",
    "NSWorkspaceSessionDidResignActiveNotification",
    "NSWorkspaceSessionDidBecomeActiveNotification",
    "NSWorkspaceWillSleepNotification",
    "NSWorkspaceDidWakeNotification",
];

impl Event {
    /// The event a macOS notification name stands for.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub fn from_notification(name: &str) -> Option<Event> {
        Some(match name {
            "com.apple.screenIsLocked" => Event::ScreenLocked,
            "com.apple.screenIsUnlocked" => Event::ScreenUnlocked,
            "com.apple.screensaver.didstart" => Event::ScreensaverStarted,
            "com.apple.screensaver.didstop" => Event::ScreensaverStopped,
            "NSWorkspaceScreensDidSleepNotification" => Event::DisplaysSlept,
            "NSWorkspaceScreensDidWakeNotification" => Event::DisplaysWoke,
            "NSWorkspaceSessionDidResignActiveNotification" => Event::SessionResigned,
            "NSWorkspaceSessionDidBecomeActiveNotification" => Event::SessionActivated,
            "NSWorkspaceWillSleepNotification" => Event::SystemWillSleep,
            "NSWorkspaceDidWakeNotification" => Event::SystemDidWake,
            _ => return None,
        })
    }
}

/// A direct reading of the system state. Each field is `None` when this
/// platform can't tell; then what events said is kept.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Probe {
    pub screen_locked: Option<bool>,
    pub screensaver: Option<bool>,
    pub display_asleep: Option<bool>,
    /// False while another user's session owns the screen.
    pub on_console: Option<bool>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Quiet {
    screen_locked: bool,
    screensaver: bool,
    display_asleep: bool,
    switched_away: bool,
    sleeping: bool,
}

impl Quiet {
    #[cfg_attr(target_os = "linux", allow(dead_code))]
    pub fn apply(&mut self, event: Event) {
        match event {
            Event::ScreenLocked => self.screen_locked = true,
            Event::ScreenUnlocked => self.screen_locked = false,
            Event::ScreensaverStarted => self.screensaver = true,
            Event::ScreensaverStopped => self.screensaver = false,
            Event::DisplaysSlept => self.display_asleep = true,
            Event::DisplaysWoke => self.display_asleep = false,
            Event::SessionResigned => self.switched_away = true,
            Event::SessionActivated => self.switched_away = false,
            Event::SystemWillSleep => self.sleeping = true,
            // Waking the Mac doesn't unlock it or wake the display; those
            // have their own events (and the next probe).
            Event::SystemDidWake => self.sleeping = false,
        }
    }

    /// A probe reads the real state, so it wins for everything it can see.
    /// (System sleep can't be probed: the app is frozen while it lasts.)
    pub fn update(&mut self, probe: Probe) {
        if let Some(locked) = probe.screen_locked {
            self.screen_locked = locked;
        }
        if let Some(running) = probe.screensaver {
            self.screensaver = running;
        }
        if let Some(asleep) = probe.display_asleep {
            self.display_asleep = asleep;
        }
        if let Some(on_console) = probe.on_console {
            self.switched_away = !on_console;
        }
    }

    /// The reason to keep quiet, if there is one.
    pub fn reason(&self) -> Option<Reason> {
        [
            (self.sleeping, Reason::SystemSleeping),
            (self.switched_away, Reason::UserSwitchedAway),
            (self.screen_locked, Reason::ScreenLocked),
            (self.screensaver, Reason::Screensaver),
            (self.display_asleep, Reason::DisplayAsleep),
        ]
        .into_iter()
        .find_map(|(active, reason)| active.then_some(reason))
    }

    #[cfg(test)]
    pub fn is_quiet(&self) -> bool {
        self.reason().is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AWAKE: Probe = Probe {
        screen_locked: Some(false),
        screensaver: Some(false),
        display_asleep: Some(false),
        on_console: Some(true),
    };

    #[test]
    fn every_subscribed_notification_maps_to_an_event() {
        let names = DISTRIBUTED_NOTIFICATIONS.iter().chain(WORKSPACE_NOTIFICATIONS.iter());
        let events: Vec<Event> = names
            .map(|n| Event::from_notification(n).unwrap_or_else(|| panic!("unmapped {n}")))
            .collect();
        // Ten distinct events: every on has its off.
        assert_eq!(events.len(), 10);
        for (i, a) in events.iter().enumerate() {
            assert!(!events[i + 1..].contains(a), "{a:?} mapped twice");
        }
        assert_eq!(Event::from_notification("com.example.unrelated"), None);
    }

    #[test]
    fn loud_by_default() {
        assert_eq!(Quiet::default().reason(), None);
        let mut q = Quiet::default();
        q.update(AWAKE);
        assert!(!q.is_quiet());
    }

    #[test]
    fn each_event_makes_it_quiet_and_its_opposite_undoes_it() {
        let pairs = [
            (Event::ScreenLocked, Event::ScreenUnlocked, Reason::ScreenLocked),
            (Event::ScreensaverStarted, Event::ScreensaverStopped, Reason::Screensaver),
            (Event::DisplaysSlept, Event::DisplaysWoke, Reason::DisplayAsleep),
            (Event::SessionResigned, Event::SessionActivated, Reason::UserSwitchedAway),
            (Event::SystemWillSleep, Event::SystemDidWake, Reason::SystemSleeping),
        ];
        for (on, off, reason) in pairs {
            let mut q = Quiet::default();
            q.apply(on);
            assert_eq!(q.reason(), Some(reason), "{on:?}");
            q.apply(off);
            assert_eq!(q.reason(), None, "{off:?}");
        }
    }

    #[test]
    fn stays_quiet_until_every_reason_is_gone() {
        // Screensaver starts, then the screen locks under it (the usual
        // order when "require password" kicks in).
        let mut q = Quiet::default();
        q.apply(Event::ScreensaverStarted);
        q.apply(Event::ScreenLocked);
        q.apply(Event::ScreensaverStopped);
        assert_eq!(q.reason(), Some(Reason::ScreenLocked), "still at the lock screen");
        q.apply(Event::ScreenUnlocked);
        assert!(!q.is_quiet());
    }

    #[test]
    fn waking_the_mac_does_not_unlock_it() {
        let mut q = Quiet::default();
        q.apply(Event::ScreenLocked);
        q.apply(Event::DisplaysSlept);
        q.apply(Event::SystemWillSleep);
        q.apply(Event::SystemDidWake);
        assert!(q.is_quiet());
        q.apply(Event::DisplaysWoke);
        assert_eq!(q.reason(), Some(Reason::ScreenLocked));
    }

    #[test]
    fn a_probe_catches_what_events_missed_and_corrects_stale_ones() {
        // App started while the screen was already locked: no event came.
        let mut q = Quiet::default();
        q.update(Probe { screen_locked: Some(true), ..AWAKE });
        assert_eq!(q.reason(), Some(Reason::ScreenLocked));
        // The unlock notification was missed, but the probe sees it.
        q.update(AWAKE);
        assert!(!q.is_quiet());

        for (probe, reason) in [
            (Probe { screensaver: Some(true), ..AWAKE }, Reason::Screensaver),
            (Probe { display_asleep: Some(true), ..AWAKE }, Reason::DisplayAsleep),
            (Probe { on_console: Some(false), ..AWAKE }, Reason::UserSwitchedAway),
        ] {
            let mut q = Quiet::default();
            q.update(probe);
            assert_eq!(q.reason(), Some(reason), "{probe:?}");
        }
    }

    #[test]
    fn a_probe_that_cannot_see_something_keeps_what_events_said() {
        // Windows can't ask whether the display is off, only hear about it.
        let mut q = Quiet::default();
        q.apply(Event::DisplaysSlept);
        q.update(Probe { display_asleep: None, ..AWAKE });
        assert_eq!(q.reason(), Some(Reason::DisplayAsleep));
        // An empty probe changes nothing at all.
        q.update(Probe::default());
        assert_eq!(q.reason(), Some(Reason::DisplayAsleep));
    }

    #[test]
    fn a_probe_cannot_cancel_system_sleep() {
        let mut q = Quiet::default();
        q.apply(Event::SystemWillSleep);
        q.update(AWAKE);
        assert_eq!(q.reason(), Some(Reason::SystemSleeping));
    }
}
