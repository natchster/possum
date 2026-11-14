package io.github.natchster.possum;

import android.content.Context;
import android.graphics.Bitmap;
import android.graphics.Canvas;
import android.graphics.Insets;
import android.graphics.Paint;
import android.graphics.PixelFormat;
import android.graphics.Rect;
import android.graphics.RectF;
import android.hardware.input.InputManager;
import android.view.Display;
import android.view.Gravity;
import android.view.View;
import android.view.WindowInsets;
import android.view.WindowManager;
import android.view.WindowMetrics;
import android.widget.FrameLayout;

/**
 * Possum's overlay on one display: a transparent window over everything
 * except the system's own (status bar, taskbar, notification shade, lock
 * screen), which taps and clicks go straight through.
 *
 * Android lets touches through another app's window only while it is at
 * most 80% opaque (InputManager.getMaximumObscuringOpacityForTouch), so the
 * whole window is drawn at that opacity: the possum is very slightly
 * see-through, and never in the way.
 */
final class Stage {
    interface Listener {
        /** The track changed size: rotation, resolution, a taskbar. */
        void onTrackChanged(Stage stage);
    }

    /** What the possum stays clear of: the system bars (status bar,
     * navigation bar or taskbar) and the camera cutout. */
    private static final int BARS = WindowInsets.Type.systemBars() | WindowInsets.Type.displayCutout();

    /** On the desktop, 3 points per sprite pixel. */
    private static final float DESKTOP_SCALE = 3f;

    /** On small screens: the running possum at most a fifth of the shorter side. */
    private static final int SHARE_OF_SHORT_SIDE = 5;

    final Display display;
    /** Pixels per sprite pixel: always whole, so the pixel art stays crisp. */
    final int scale;
    final float density;
    /** The area the possum runs around, in window pixels. */
    final Rect track = new Rect();

    private final WindowManager windows;
    private final Root root;
    private final Picture possum, bubble;
    private final Bitmap[] pictures;
    private final float feetX, feetY;
    private final Listener listener;

    /** Throws if the display won't take an overlay. */
    Stage(Context base, Display display, Bitmap[] pictures, float[] layout, int runWidth, Listener listener) {
        this.display = display;
        this.pictures = pictures;
        this.listener = listener;
        Context context = base.createWindowContext(display, WindowManager.LayoutParams.TYPE_APPLICATION_OVERLAY, null);
        windows = context.getSystemService(WindowManager.class);
        WindowMetrics metrics = windows.getMaximumWindowMetrics();
        Rect bounds = metrics.getBounds();
        setTrack(bounds.width(), bounds.height(), metrics.getWindowInsets().getInsets(BARS));

        density = context.getResources().getDisplayMetrics().density;
        int shortSide = Math.min(bounds.width(), bounds.height());
        int fits = shortSide / (SHARE_OF_SHORT_SIDE * runWidth);
        scale = Math.max(1, Math.min(Math.round(DESKTOP_SCALE * density), fits));
        feetX = layout[Native.FEET_X] * scale;
        feetY = layout[Native.FEET_Y] * scale;

        root = new Root(context);
        possum = new Picture(context);
        bubble = new Picture(context);
        root.addView(possum, size(layout[Native.POSSUM_W], layout[Native.POSSUM_H]));
        root.addView(bubble, size(layout[Native.BUBBLE_MAX_W], layout[Native.BUBBLE_MAX_H]));
        // It turns around its paws, which stay on the track.
        possum.setPivotX(feetX);
        possum.setPivotY(feetY);
        possum.setVisibility(View.INVISIBLE);
        bubble.setVisibility(View.INVISIBLE);
        windows.addView(root, params(context));
    }

    private FrameLayout.LayoutParams size(float w, float h) {
        return new FrameLayout.LayoutParams(Math.round(w * scale), Math.round(h * scale), Gravity.TOP | Gravity.START);
    }

    private static WindowManager.LayoutParams params(Context context) {
        WindowManager.LayoutParams p = new WindowManager.LayoutParams(
                WindowManager.LayoutParams.MATCH_PARENT,
                WindowManager.LayoutParams.MATCH_PARENT,
                WindowManager.LayoutParams.TYPE_APPLICATION_OVERLAY,
                WindowManager.LayoutParams.FLAG_NOT_TOUCHABLE
                        | WindowManager.LayoutParams.FLAG_NOT_FOCUSABLE
                        | WindowManager.LayoutParams.FLAG_LAYOUT_IN_SCREEN
                        | WindowManager.LayoutParams.FLAG_HARDWARE_ACCELERATED,
                PixelFormat.TRANSLUCENT);
        p.gravity = Gravity.TOP | Gravity.START;
        p.setTitle("Possum");
        // The whole display, under the bars too; the track keeps clear of them.
        p.setFitInsetsTypes(0);
        p.layoutInDisplayCutoutMode = WindowManager.LayoutParams.LAYOUT_IN_DISPLAY_CUTOUT_MODE_ALWAYS;
        p.alpha = context.getSystemService(InputManager.class).getMaximumObscuringOpacityForTouch();
        return p;
    }

    private void setTrack(int width, int height, Insets bars) {
        track.set(bars.left, bars.top, width - bars.right, height - bars.bottom);
    }

    /** Draws a tick's scene (see Native). */
    void draw(float[] scene) {
        // The brain's coordinates are y-up from the bottom-left of the track.
        float pawsX = track.left + scene[Native.X];
        float pawsY = track.bottom - scene[Native.Y];
        Bitmap picture = pictures[(int) scene[Native.POSSUM]];
        possum.show(picture, picture.getWidth() * scale, picture.getHeight() * scale);
        possum.setTranslationX(pawsX - feetX);
        possum.setTranslationY(pawsY - feetY);
        possum.setRotation((float) -Math.toDegrees(scene[Native.HEADING]));
        possum.setVisibility(View.VISIBLE);

        int b = (int) scene[Native.BUBBLE];
        if (b >= 0) {
            bubble.show(pictures[b], scene[Native.BUBBLE_W], scene[Native.BUBBLE_H]);
            bubble.setTranslationX(track.left + scene[Native.BUBBLE_LEFT]);
            bubble.setTranslationY(track.bottom - scene[Native.BUBBLE_BOTTOM] - scene[Native.BUBBLE_H]);
            bubble.setVisibility(View.VISIBLE);
        } else {
            bubble.setVisibility(View.INVISIBLE);
        }
    }

    void close() {
        try {
            windows.removeViewImmediate(root);
        } catch (RuntimeException e) {
            // Already gone with its display.
        }
    }

    String describe() {
        return display.getName() + " (display " + display.getDisplayId() + "), track " + track.width() + "×"
                + track.height() + " px, " + scale + " px per sprite pixel, density " + density;
    }

    /**
     * The window's content: follows size and system bar changes. On a
     * rotation the new insets come before the new size, so it waits for
     * both and reports the track once.
     */
    private final class Root extends FrameLayout {
        private final Runnable settle = this::settle;

        Root(Context context) {
            super(context);
        }

        @Override
        public WindowInsets onApplyWindowInsets(WindowInsets insets) {
            settleSoon();
            return super.onApplyWindowInsets(insets);
        }

        @Override
        protected void onSizeChanged(int w, int h, int oldW, int oldH) {
            super.onSizeChanged(w, h, oldW, oldH);
            settleSoon();
        }

        private void settleSoon() {
            removeCallbacks(settle);
            post(settle);
        }

        private void settle() {
            WindowInsets insets = getRootWindowInsets();
            if (getWidth() <= 0 || getHeight() <= 0 || insets == null) return;
            Rect before = new Rect(track);
            setTrack(getWidth(), getHeight(), insets.getInsets(BARS));
            if (!before.equals(track)) listener.onTrackChanged(Stage.this);
        }
    }

    /** A picture scaled up by whole pixels, without smoothing. */
    private static final class Picture extends View {
        private final Paint paint = new Paint();
        private final Rect from = new Rect();
        private final RectF to = new RectF();
        private Bitmap bitmap;

        Picture(Context context) {
            super(context);
            paint.setFilterBitmap(false);
            paint.setAntiAlias(false);
            paint.setDither(false);
        }

        void show(Bitmap b, float w, float h) {
            if (b == bitmap && to.right == w && to.bottom == h) return;
            bitmap = b;
            from.set(0, 0, b.getWidth(), b.getHeight());
            to.set(0, 0, w, h);
            invalidate();
        }

        @Override
        protected void onDraw(Canvas canvas) {
            if (bitmap != null) canvas.drawBitmap(bitmap, from, to, paint);
        }
    }
}
