package io.github.natchster.possum;

import android.app.KeyguardManager;
import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.PendingIntent;
import android.app.Service;
import android.content.BroadcastReceiver;
import android.content.Context;
import android.content.Intent;
import android.content.IntentFilter;
import android.content.SharedPreferences;
import android.content.pm.ServiceInfo;
import android.graphics.Bitmap;
import android.graphics.drawable.Icon;
import android.hardware.display.DisplayManager;
import android.media.AudioManager;
import android.os.Build;
import android.os.Handler;
import android.os.IBinder;
import android.os.Looper;
import android.os.PowerManager;
import android.provider.Settings;
import android.util.Log;
import android.view.Choreographer;
import android.view.Display;

import java.util.HashSet;
import java.util.Set;

/**
 * Possum itself: a foreground service (Android's way of saying "keep this
 * running, and show a notification about it") that keeps the overlay on the
 * best display, ticks the brain every frame, and plays the screams.
 *
 * The best display is a connected monitor (Samsung DeX, Android's desktop
 * mode) when there is one, and this device's own screen otherwise. If a
 * monitor won't take the overlay, it stays on this screen until that
 * monitor is unplugged and plugged in again.
 *
 * The notification is the menu, as on the desktop: Scream / Scream
 * silently, Scream Now, Quit Possum.
 */
public final class PossumService extends Service
        implements Choreographer.FrameCallback, Stage.Listener, DisplayManager.DisplayListener {
    static final String TAG = "possum";

    static final String START = "io.github.natchster.possum.START";
    static final String TOGGLE_SILENT = "io.github.natchster.possum.TOGGLE_SILENT";
    static final String SET_SILENT = "io.github.natchster.possum.SET_SILENT";
    static final String SCREAM_NOW = "io.github.natchster.possum.SCREAM_NOW";
    static final String QUIT = "io.github.natchster.possum.QUIT";
    static final String EXTRA_SILENT = "silent";

    static final String PREFS = "possum";
    static final String PREF_SILENT = "silent";

    private static final String CHANNEL = "possum";
    private static final int NOTIFICATION = 1;

    /** Desktop speed: 260 points a second at 3 points per sprite pixel. */
    private static final float SPEED_PER_SCALE = 260f / 3f;
    /** At most about 60 frames a second, even on faster screens. */
    private static final long MIN_FRAME_NANOS = 11_000_000L;
    /** Ticks without the display's frame clock after this long. */
    private static final long STALL_NANOS = 150_000_000L;

    /** For the app screen: where it's running, or null when it isn't. */
    static volatile String status;
    /** For the app screen: why it isn't running, or what went wrong. */
    static volatile String problem;

    private Handler handler;
    private Choreographer choreographer;
    private DisplayManager displays;
    private KeyguardManager keyguard;
    private PowerManager power;
    private AudioManager audio;
    private NotificationManager notifications;
    private SharedPreferences prefs;

    private Bitmap[] pictures;
    private Bitmap notificationIcon;
    private float[] layout;
    private Voices voices;
    private Stage stage;

    private final float[] scene = new float[Native.SCENE];
    private final int[] requests = new int[Native.MAX_REQUESTS];
    private final Set<Integer> refused = new HashSet<>();
    private boolean running;
    private long startNanos, lastFrameNanos;
    private int quiet;
    private boolean userSilent, phoneSilent;

    // -----------------------------------------------------------------------
    // Lifecycle
    // -----------------------------------------------------------------------

    @Override
    public void onCreate() {
        super.onCreate();
        handler = new Handler(Looper.getMainLooper());
        choreographer = Choreographer.getInstance();
        displays = getSystemService(DisplayManager.class);
        keyguard = getSystemService(KeyguardManager.class);
        power = getSystemService(PowerManager.class);
        audio = getSystemService(AudioManager.class);
        notifications = getSystemService(NotificationManager.class);
        prefs = getSharedPreferences(PREFS, MODE_PRIVATE);
        userSilent = prefs.getBoolean(PREF_SILENT, false);

        Native.init();
        pictures = loadPictures();
        layout = Native.layout();
        Bitmap head = pictures[Native.statusIcon()];
        notificationIcon = Bitmap.createScaledBitmap(head, head.getWidth() * 4, head.getHeight() * 4, false);
        voices = new Voices();
        notifications.createNotificationChannel(new NotificationChannel(
                CHANNEL, "Possum is running", NotificationManager.IMPORTANCE_LOW));
    }

    static Bitmap[] loadPictures() {
        Bitmap[] out = new Bitmap[Native.pictureCount()];
        for (int i = 0; i < out.length; i++) {
            int[] p = Native.picture(i);
            out[i] = Bitmap.createBitmap(p, 2, p[0], p[0], p[1], Bitmap.Config.ARGB_8888);
        }
        return out;
    }

    @Override
    public int onStartCommand(Intent intent, int flags, int startId) {
        String action = intent == null || intent.getAction() == null ? START : intent.getAction();
        switch (action) {
            case QUIT:
                Log.i(TAG, "quit");
                stopSelf();
                break;
            case TOGGLE_SILENT:
                setSilent(!userSilent);
                break;
            case SET_SILENT:
                setSilent(intent.getBooleanExtra(EXTRA_SILENT, userSilent));
                break;
            case SCREAM_NOW:
                Native.screamNow();
                break;
            default:
                start();
        }
        return START_NOT_STICKY;
    }

    private void start() {
        // Android wants the notification up within seconds of being asked to
        // start, even when it's about to give up.
        goForeground();
        if (running) return;
        if (!Settings.canDrawOverlays(this)) {
            problem = "Possum isn't allowed to appear on top of other apps yet.";
            Log.w(TAG, problem);
            stopSelf();
            return;
        }
        problem = null;
        displays.registerDisplayListener(this, handler);
        IntentFilter filter = new IntentFilter();
        filter.addAction(Intent.ACTION_SCREEN_OFF);
        filter.addAction(Intent.ACTION_SCREEN_ON);
        filter.addAction(Intent.ACTION_USER_PRESENT);
        filter.addAction(AudioManager.RINGER_MODE_CHANGED_ACTION);
        filter.addAction(NotificationManager.ACTION_INTERRUPTION_FILTER_CHANGED);
        if (Build.VERSION.SDK_INT >= 33) {
            registerReceiver(systemEvents, filter, Context.RECEIVER_NOT_EXPORTED);
        } else {
            registerReceiver(systemEvents, filter);
        }
        running = true;
        startNanos = System.nanoTime();
        showOnBestDisplay();
        if (stage == null) {
            stopSelf();
            return;
        }
        probe();
        choreographer.postFrameCallback(this);
        handler.postDelayed(watchdog, 100);
    }

    @Override
    public void onDestroy() {
        Log.i(TAG, "stopping");
        if (running) {
            running = false;
            choreographer.removeFrameCallback(this);
            displays.unregisterDisplayListener(this);
            unregisterReceiver(systemEvents);
        }
        handler.removeCallbacksAndMessages(null);
        if (stage != null) stage.close();
        stage = null;
        voices.hush();
        voices.release();
        Native.stop();
        status = null;
        super.onDestroy();
    }

    @Override
    public IBinder onBind(Intent intent) {
        return null;
    }

    // -----------------------------------------------------------------------
    // Displays
    // -----------------------------------------------------------------------

    /** A connected monitor if there is one that takes the overlay, else this screen. */
    private void showOnBestDisplay() {
        Display best = displays.getDisplay(Display.DEFAULT_DISPLAY);
        long bestArea = 0;
        StringBuilder seen = new StringBuilder("displays:");
        for (Display d : displays.getDisplays()) {
            int id = d.getDisplayId();
            seen.append(String.format(" [%d \"%s\" %dx%d flags=0x%x state=%d%s]", id, d.getName(),
                    d.getMode().getPhysicalWidth(), d.getMode().getPhysicalHeight(), d.getFlags(), d.getState(),
                    refused.contains(id) ? " refused" : ""));
            if (id == Display.DEFAULT_DISPLAY || refused.contains(id) || !d.isValid()) continue;
            // A monitor that's asleep still counts: the possum waits there,
            // quietly, as on the desktop.
            if ((d.getFlags() & Display.FLAG_PRIVATE) != 0) continue;
            long area = (long) d.getMode().getPhysicalWidth() * d.getMode().getPhysicalHeight();
            if (area > bestArea) {
                best = d;
                bestArea = area;
            }
        }
        Log.i(TAG, seen.toString());
        if (stage != null && stage.display.getDisplayId() == best.getDisplayId() && stage.display.isValid()
                && density(best) == stage.density) {
            return;
        }
        Stage next = open(best);
        if (next == null && best.getDisplayId() != Display.DEFAULT_DISPLAY) {
            refused.add(best.getDisplayId());
            problem = "“" + best.getName() + "” wouldn't show Possum, so it runs on this screen.";
            next = open(displays.getDisplay(Display.DEFAULT_DISPLAY));
        }
        if (next == null) {
            if (stage == null) problem = "Android wouldn't show Possum on any screen.";
            return;
        }
        if (stage != null) stage.close();
        stage = next;
        if (next.display.getDisplayId() == best.getDisplayId()) problem = null;
        Native.run(stage.scale, SPEED_PER_SCALE * stage.scale, stage.track.width(), stage.track.height());
        status = stage.display.getDisplayId() == Display.DEFAULT_DISPLAY
                ? "on this screen" : "on the connected display “" + stage.display.getName() + "”";
        Log.i(TAG, "showing on " + stage.describe());
        goForeground();
    }

    private float density(Display display) {
        return createDisplayContext(display).getResources().getDisplayMetrics().density;
    }

    private Stage open(Display display) {
        try {
            return new Stage(this, display, pictures, layout, pictures[0].getWidth(), this);
        } catch (RuntimeException e) {
            Log.w(TAG, "display " + display.getDisplayId() + " (" + display.getName() + ") refused the overlay", e);
            return null;
        }
    }

    private final Runnable recheck = this::showOnBestDisplay;

    private void recheckSoon() {
        if (!running) return;
        handler.removeCallbacks(recheck);
        // Displays come and go in bursts (DeX starting, a dock waking up).
        handler.postDelayed(recheck, 500);
    }

    @Override
    public void onDisplayAdded(int id) {
        Log.i(TAG, "display " + id + " added");
        recheckSoon();
    }

    @Override
    public void onDisplayRemoved(int id) {
        Log.i(TAG, "display " + id + " removed");
        refused.remove(id);
        recheckSoon();
    }

    @Override
    public void onDisplayChanged(int id) {
        recheckSoon();
        if (stage != null && id == stage.display.getDisplayId()) probe();
    }

    @Override
    public void onTrackChanged(Stage changed) {
        if (changed != stage) return;
        Log.i(TAG, "track now " + stage.track.width() + "×" + stage.track.height());
        Native.resize(stage.track.width(), stage.track.height());
    }

    // -----------------------------------------------------------------------
    // Frames
    // -----------------------------------------------------------------------

    @Override
    public void doFrame(long frameTimeNanos) {
        if (!running) return;
        choreographer.postFrameCallback(this);
        if (frameTimeNanos - lastFrameNanos >= MIN_FRAME_NANOS) frame(frameTimeNanos);
    }

    /**
     * The frame clock follows this device's own screen. If that screen is
     * off while a monitor is on (DeX with the phone screen off), tick from a
     * timer instead.
     */
    private final Runnable watchdog = new Runnable() {
        @Override
        public void run() {
            if (!running) return;
            long now = System.nanoTime();
            boolean stalled = now - lastFrameNanos > STALL_NANOS && (quiet & Native.DISPLAY_OFF) == 0;
            if (stalled) frame(now);
            handler.postDelayed(this, stalled ? 16 : 100);
        }
    };

    private void frame(long nanos) {
        lastFrameNanos = nanos;
        if (stage == null) return;
        double now = (nanos - startNanos) / 1e9;
        int n = Native.tick(now, userSilent || phoneSilent, quiet, scene, requests);
        for (int i = 0; i < n; i++) {
            int r = requests[i];
            if (r == Native.HUSH) {
                voices.hush();
            } else if ((r & Native.PLAY) != 0) {
                voices.play((r >> 8) & 0xFF, r & 0xFF);
            }
        }
        stage.draw(scene);
        if (scene[Native.CHECK_DISPLAYS] != 0) probe();
    }

    // -----------------------------------------------------------------------
    // When to keep quiet
    // -----------------------------------------------------------------------

    /**
     * Reads the lock and display state (the brain keeps quiet while either
     * is set) and whether the phone wants silence: silent or vibrate mode,
     * Do Not Disturb, or a call. Then the possum screams silently, in
     * speech bubbles, whatever the menu says.
     */
    private void probe() {
        if (stage == null) return;
        int q = 0;
        if (keyguard.isKeyguardLocked()) q |= Native.LOCKED;
        int state = stage.display.getState();
        boolean off = state == Display.STATE_OFF || state == Display.STATE_DOZE || state == Display.STATE_DOZE_SUSPEND;
        if (stage.display.getDisplayId() == Display.DEFAULT_DISPLAY && !power.isInteractive()) off = true;
        if (off) q |= Native.DISPLAY_OFF;
        quiet = q;
        boolean silent = audio.getRingerMode() != AudioManager.RINGER_MODE_NORMAL
                || notifications.getCurrentInterruptionFilter() > NotificationManager.INTERRUPTION_FILTER_ALL
                || audio.getMode() != AudioManager.MODE_NORMAL;
        if (silent != phoneSilent) {
            phoneSilent = silent;
            Log.i(TAG, silent ? "the phone wants quiet: screaming silently" : "the phone may make sounds again");
            goForeground();
        }
    }

    private final BroadcastReceiver systemEvents = new BroadcastReceiver() {
        @Override
        public void onReceive(Context context, Intent intent) {
            if (Intent.ACTION_SCREEN_OFF.equals(intent.getAction())) voices.hush();
            probe();
        }
    };

    // -----------------------------------------------------------------------
    // The menu
    // -----------------------------------------------------------------------

    private void setSilent(boolean silent) {
        userSilent = silent;
        prefs.edit().putBoolean(PREF_SILENT, silent).apply();
        Log.i(TAG, silent ? "scream silently" : "scream out loud");
        if (running) goForeground();
    }

    static boolean isRunning() {
        return status != null;
    }

    private void goForeground() {
        String mode = userSilent ? "screaming silently"
                : phoneSilent ? "screaming silently while the phone is quiet" : "screaming out loud";
        String where = status == null ? "Starting…"
                : Character.toUpperCase(status.charAt(0)) + status.substring(1) + ", " + mode + ".";
        PendingIntent open = PendingIntent.getActivity(this, 0, new Intent(this, MainActivity.class),
                PendingIntent.FLAG_IMMUTABLE | PendingIntent.FLAG_UPDATE_CURRENT);
        Notification n = new Notification.Builder(this, CHANNEL)
                .setSmallIcon(Icon.createWithBitmap(notificationIcon))
                .setContentTitle("Possum is running!")
                .setContentText(where)
                .setContentIntent(open)
                .setOngoing(true)
                .setShowWhen(false)
                .setCategory(Notification.CATEGORY_SERVICE)
                .setForegroundServiceBehavior(Notification.FOREGROUND_SERVICE_IMMEDIATE)
                .addAction(action(userSilent ? "Scream" : "Scream silently", TOGGLE_SILENT, 1))
                .addAction(action("Scream Now", SCREAM_NOW, 2))
                .addAction(action("Quit Possum", QUIT, 3))
                .build();
        if (Build.VERSION.SDK_INT >= 34) {
            startForeground(NOTIFICATION, n, ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE);
        } else {
            startForeground(NOTIFICATION, n);
        }
    }

    private Notification.Action action(String title, String action, int request) {
        Intent intent = new Intent(this, PossumService.class).setAction(action);
        PendingIntent pending = PendingIntent.getService(this, request, intent,
                PendingIntent.FLAG_IMMUTABLE | PendingIntent.FLAG_UPDATE_CURRENT);
        return new Notification.Action.Builder(null, title, pending).build();
    }
}
