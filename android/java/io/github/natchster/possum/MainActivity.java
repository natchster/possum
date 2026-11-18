package io.github.natchster.possum;

import android.Manifest;
import android.app.Activity;
import android.content.Intent;
import android.content.pm.PackageManager;
import android.graphics.Bitmap;
import android.graphics.Insets;
import android.graphics.drawable.BitmapDrawable;
import android.net.Uri;
import android.os.Build;
import android.os.Bundle;
import android.os.Handler;
import android.os.Looper;
import android.provider.Settings;
import android.util.DisplayMetrics;
import android.util.TypedValue;
import android.view.Gravity;
import android.view.View;
import android.view.Window;
import android.view.WindowInsets;
import android.widget.Button;
import android.widget.ImageView;
import android.widget.LinearLayout;
import android.widget.ScrollView;
import android.widget.Switch;
import android.widget.TextView;

/**
 * The app's one screen: starts Possum (opening the app starts it, as on the
 * desktop), asks for the one permission it needs, and says where it runs.
 */
public final class MainActivity extends Activity {
    private final Handler handler = new Handler(Looper.getMainLooper());
    private TextView status;
    private Button allow, startStop;
    private Switch silent;
    private boolean startWhenAllowed;

    private final Runnable refresh = new Runnable() {
        @Override
        public void run() {
            update();
            handler.postDelayed(this, 1000);
        }
    };

    @Override
    protected void onCreate(Bundle saved) {
        super.onCreate(saved);
        requestWindowFeature(Window.FEATURE_NO_TITLE);
        startWhenAllowed = saved == null;
        setContentView(build());
        if (Build.VERSION.SDK_INT >= 33
                && checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED) {
            // The notification is Possum's menu; it runs without it too.
            requestPermissions(new String[] {Manifest.permission.POST_NOTIFICATIONS}, 1);
        }
    }

    @Override
    protected void onResume() {
        super.onResume();
        if (startWhenAllowed && Settings.canDrawOverlays(this)) {
            startWhenAllowed = false;
            start();
        }
        handler.post(refresh);
    }

    @Override
    protected void onPause() {
        handler.removeCallbacks(refresh);
        super.onPause();
    }

    private void start() {
        startForegroundService(new Intent(this, PossumService.class).setAction(PossumService.START));
    }

    private void update() {
        boolean allowed = Settings.canDrawOverlays(this);
        boolean running = PossumService.isRunning();
        String problem = PossumService.problem;
        String text;
        if (!allowed) {
            text = "Possum needs your permission to appear on top of other apps.";
        } else if (running) {
            text = "Possum is running " + PossumService.status + ".";
            String quiet = PossumService.quietMode(this, PossumService.hushNow);
            if (quiet != null) {
                text += "\nIt screams silently while " + quiet + ".";
            } else if (PossumService.mediaOff) {
                text += "\nThe media volume is off, so you won't hear it scream.";
            }
            if (Voices.route != null) text += "\nIts screams play through " + Voices.route + ".";
        } else {
            text = "Possum isn't running.";
        }
        if (problem != null && allowed) text += "\n" + problem;
        status.setText(text);
        allow.setVisibility(allowed ? View.GONE : View.VISIBLE);
        startStop.setEnabled(allowed);
        startStop.setText(running ? "Stop Possum" : "Start Possum");
        // While the phone asks for quiet, it screams silently whatever was picked.
        silent.setChecked(PossumService.hushNow != Native.HUSH_NONE
                || getSharedPreferences(PossumService.PREFS, MODE_PRIVATE).getBoolean(PossumService.PREF_SILENT, false));
    }

    // -----------------------------------------------------------------------
    // The layout, built in code (the app has no layout files)
    // -----------------------------------------------------------------------

    private View build() {
        LinearLayout column = new LinearLayout(this);
        column.setOrientation(LinearLayout.VERTICAL);
        int pad = dp(24);

        ImageView portrait = new ImageView(this);
        Native.init();
        // The first scream frame, crisp at 4 dp per sprite pixel.
        int[] p = Native.picture(Native.portrait());
        Bitmap bitmap = Bitmap.createBitmap(p, 2, p[0], p[0], p[1], Bitmap.Config.ARGB_8888);
        BitmapDrawable drawable = new BitmapDrawable(getResources(), bitmap);
        drawable.setFilterBitmap(false);
        portrait.setImageDrawable(drawable);
        portrait.setAdjustViewBounds(true);
        portrait.setScaleType(ImageView.ScaleType.FIT_START);
        column.addView(portrait, new LinearLayout.LayoutParams(dp(4 * p[0]), dp(4 * p[1])));

        column.addView(text("Possum", 28, true));
        column.addView(text("A screaming pixel-art possum runs around the edge of your screen. With Samsung DeX "
                + "or a connected monitor, it runs around the monitor.", 16, false));

        status = text("", 16, true);
        status.setPadding(0, dp(16), 0, dp(8));
        column.addView(status);

        allow = button("Allow Possum to appear on top", v -> startActivity(new Intent(
                Settings.ACTION_MANAGE_OVERLAY_PERMISSION, Uri.parse("package:" + getPackageName()))));
        column.addView(allow);
        startStop = button("Start Possum", v -> {
            if (PossumService.isRunning()) {
                stopService(new Intent(this, PossumService.class));
            } else {
                start();
            }
            handler.postDelayed(this::update, 300);
        });
        column.addView(startStop);

        silent = new Switch(this);
        silent.setText("Scream silently");
        silent.setTextSize(TypedValue.COMPLEX_UNIT_SP, 16);
        silent.setPadding(0, dp(12), 0, dp(12));
        silent.setOnClickListener(v -> {
            boolean on = silent.isChecked();
            getSharedPreferences(PossumService.PREFS, MODE_PRIVATE).edit()
                    .putBoolean(PossumService.PREF_SILENT, on).apply();
            if (PossumService.isRunning()) {
                startService(new Intent(this, PossumService.class).setAction(PossumService.SET_SILENT)
                        .putExtra(PossumService.EXTRA_SILENT, on));
                if (!on && PossumService.hushNow != Native.HUSH_NONE) {
                    PossumService.tellWhySilent(this);
                    silent.setChecked(true);
                }
            }
        });
        column.addView(silent);

        column.addView(text("Taps and clicks go straight through the possum: Android only allows that for "
                + "windows that are slightly see-through, so it is.", 14, false));
        column.addView(text("While the phone is set to silent or vibrate or to Do Not Disturb, it screams "
                + "silently, in speech bubbles: Possum respects you. Scream Now, in its notification, still "
                + "screams out loud (but not on a call). Its screams play at the media volume.", 14, false));
        column.addView(text("Can't switch on “appear on top”? Android blocks it for apps installed from a "
                + "file. Open App info, tap ⋮, choose “Allow restricted settings”, then try again.", 14, false));
        column.addView(text("If Possum stops by itself after a while, set App info → Battery → Unrestricted.",
                14, false));
        column.addView(button("Open App info", v -> startActivity(new Intent(
                Settings.ACTION_APPLICATION_DETAILS_SETTINGS, Uri.parse("package:" + getPackageName())))));

        ScrollView scroll = new ScrollView(this);
        scroll.addView(column);
        // Keep clear of the status bar, navigation bar and cutouts.
        scroll.setOnApplyWindowInsetsListener((v, insets) -> {
            Insets bars = insets.getInsets(WindowInsets.Type.systemBars() | WindowInsets.Type.displayCutout());
            v.setPadding(bars.left + pad, bars.top + pad, bars.right + pad, bars.bottom + pad);
            return WindowInsets.CONSUMED;
        });
        return scroll;
    }

    private TextView text(String s, int sp, boolean bold) {
        TextView t = new TextView(this);
        t.setText(s);
        if (bold) {
            // Headings in the theme's main text colour; the rest stays softer.
            t.setTextAppearance(android.R.style.TextAppearance_Material_Body1);
            t.setTypeface(t.getTypeface(), android.graphics.Typeface.BOLD);
        }
        t.setTextSize(TypedValue.COMPLEX_UNIT_SP, sp);
        t.setPadding(0, dp(6), 0, dp(6));
        return t;
    }

    private Button button(String label, View.OnClickListener onClick) {
        Button b = new Button(this);
        b.setText(label);
        b.setAllCaps(false);
        b.setOnClickListener(onClick);
        b.setGravity(Gravity.CENTER);
        return b;
    }

    private int dp(float v) {
        DisplayMetrics metrics = getResources().getDisplayMetrics();
        return Math.round(TypedValue.applyDimension(TypedValue.COMPLEX_UNIT_DIP, v, metrics));
    }
}
