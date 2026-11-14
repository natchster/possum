package io.github.natchster.possum;

/**
 * The shared Rust core in libpossum.so (android/src/lib.rs): the desktop
 * app's brain, sprites and screams. Called from the main thread only.
 */
final class Native {
    static {
        System.loadLibrary("possum");
    }

    private Native() {}

    // The scene array tick() fills in (SCENE_LEN in lib.rs): the paws, from
    // the bottom-left of the track, y-up; heading; picture numbers; the
    // bubble's place; whether to look at the displays again; screaming.
    static final int X = 0, Y = 1, HEADING = 2, POSSUM = 3, BUBBLE = 4, BUBBLE_LEFT = 5, BUBBLE_BOTTOM = 6,
            BUBBLE_W = 7, BUBBLE_H = 8, CHECK_DISPLAYS = 9, SCREAMING = 10, SCENE = 11;

    /** Sound requests from tick(): stop everything, or PLAY | kind << 8 | variant. */
    static final int HUSH = 1 << 16, PLAY = 2 << 16, MAX_REQUESTS = 8;

    /** The quiet bits for tick(): no sound while either is set. */
    static final int LOCKED = 1, DISPLAY_OFF = 2;

    // layout(): where the paws are on the pictures, and the view sizes, in
    // sprite pixels.
    static final int FEET_X = 0, FEET_Y = 1, POSSUM_W = 2, POSSUM_H = 3, BUBBLE_MAX_W = 4, BUBBLE_MAX_H = 5;

    /** Draws the pictures and synthesises the screams, once per process. */
    static native void init();

    /** A fresh possum on a width × height track, in pixels. */
    static native void run(float scale, float speed, float width, float height);

    static native void stop();

    /** The track changed size: carry on from the same spot. */
    static native void resize(float width, float height);

    /** One frame; returns how many sound requests it wrote. */
    static native int tick(double now, boolean silent, int quiet, float[] scene, int[] requests);

    static native void screamNow();

    static native int pictureCount();

    /** Picture number index as {width, height, colours...}. */
    static native int[] picture(int index);

    static native int statusIcon();

    static native float[] layout();

    static native int sampleRate();

    static native int voiceCount(int kind);

    static native short[] voice(int kind, int variant);
}
