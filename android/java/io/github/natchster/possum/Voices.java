package io.github.natchster.possum;

import android.media.AudioAttributes;
import android.media.AudioDeviceInfo;
import android.media.AudioFormat;
import android.media.AudioRouting;
import android.media.AudioTrack;
import android.util.Log;

/**
 * The screams, ready to play: one preloaded AudioTrack per variation. They
 * play as media, at the media volume, through wherever Android sends media:
 * with DeX or a monitor that's often the monitor (HDMI), so the log says
 * where. Android can refuse to make one (its audio service still starting
 * up, say): then it's made again when that scream is due.
 */
final class Voices {
    /** As loud as the desktop app's default. */
    private static final float VOLUME = 0.6f;

    /** For the app screen: where the screams last played, or null. */
    static volatile String route;

    private final AudioAttributes attributes = new AudioAttributes.Builder()
            .setUsage(AudioAttributes.USAGE_MEDIA)
            .setContentType(AudioAttributes.CONTENT_TYPE_SONIFICATION)
            .build();
    private final AudioFormat format = new AudioFormat.Builder()
            .setSampleRate(Native.sampleRate())
            .setEncoding(AudioFormat.ENCODING_PCM_16BIT)
            .setChannelMask(AudioFormat.CHANNEL_OUT_MONO)
            .build();
    private final short[][][] samples = new short[2][][];
    private final AudioTrack[][] tracks = new AudioTrack[2][];
    /** The last try to make or play a scream failed (said once, not every time). */
    private boolean failing;
    private final AudioRouting.OnRoutingChangedListener routed = router -> {
        AudioDeviceInfo device = router.getRoutedDevice();
        String where = device == null ? null : describe(device);
        if (where != null && !where.equals(route)) {
            route = where;
            Log.i(PossumService.TAG, "its screams play through " + where);
        }
    };

    Voices() {
        for (int kind = 0; kind < tracks.length; kind++) {
            int count = Native.voiceCount(kind);
            samples[kind] = new short[count][];
            tracks[kind] = new AudioTrack[count];
            for (int variant = 0; variant < count; variant++) {
                samples[kind][variant] = Native.voice(kind, variant);
                tracks[kind][variant] = make(samples[kind][variant]);
            }
        }
    }

    /** A track that plays these samples, or null if Android won't make one now. */
    private AudioTrack make(short[] pcm) {
        if (pcm == null) return null;
        try {
            AudioTrack track = new AudioTrack.Builder()
                    .setAudioAttributes(attributes)
                    .setAudioFormat(format)
                    .setTransferMode(AudioTrack.MODE_STATIC)
                    .setBufferSizeInBytes(pcm.length * 2)
                    .build();
            track.write(pcm, 0, pcm.length);
            track.setVolume(VOLUME);
            track.addOnRoutingChangedListener(routed, null);
            return track;
        } catch (RuntimeException e) {
            failed("can't prepare a scream", e);
            return null;
        }
    }

    /** Plays a scream from the start; false if it can't. */
    boolean play(int kind, int variant) {
        if (kind < 0 || kind >= tracks.length || variant < 0 || variant >= tracks[kind].length) return false;
        AudioTrack track = tracks[kind][variant];
        if (track == null) {
            track = tracks[kind][variant] = make(samples[kind][variant]);
            if (track == null) return false;
        }
        try {
            track.pause();
            track.reloadStaticData();
            track.play();
        } catch (IllegalStateException e) {
            failed("can't play a scream", e);
            track.removeOnRoutingChangedListener(routed);
            track.release();
            tracks[kind][variant] = null;
            return false;
        }
        if (failing) {
            failing = false;
            Log.i(PossumService.TAG, "the screams play again");
        }
        return true;
    }

    private void failed(String what, RuntimeException e) {
        if (failing) return;
        failing = true;
        Log.w(PossumService.TAG, what + ", so it's silent; trying again at the next scream", e);
    }

    void hush() {
        for (AudioTrack[] kind : tracks) {
            for (AudioTrack track : kind) {
                if (track != null && track.getPlayState() == AudioTrack.PLAYSTATE_PLAYING) track.pause();
            }
        }
    }

    void release() {
        for (AudioTrack[] kind : tracks) {
            for (AudioTrack track : kind) {
                if (track == null) continue;
                track.removeOnRoutingChangedListener(routed);
                track.release();
            }
        }
        route = null;
    }

    /** "the speaker", "HDMI (LG HDR 4K)"... */
    static String describe(AudioDeviceInfo device) {
        String kind;
        switch (device.getType()) {
            case AudioDeviceInfo.TYPE_BUILTIN_SPEAKER:
            case AudioDeviceInfo.TYPE_BUILTIN_SPEAKER_SAFE:
                return "the speaker";
            case AudioDeviceInfo.TYPE_BUILTIN_EARPIECE:
                return "the earpiece";
            case AudioDeviceInfo.TYPE_HDMI:
            case AudioDeviceInfo.TYPE_HDMI_ARC:
            case AudioDeviceInfo.TYPE_HDMI_EARC:
                kind = "HDMI";
                break;
            case AudioDeviceInfo.TYPE_BLUETOOTH_A2DP:
            case AudioDeviceInfo.TYPE_BLE_HEADSET:
            case AudioDeviceInfo.TYPE_BLE_SPEAKER:
                kind = "Bluetooth";
                break;
            case AudioDeviceInfo.TYPE_USB_DEVICE:
            case AudioDeviceInfo.TYPE_USB_HEADSET:
            case AudioDeviceInfo.TYPE_USB_ACCESSORY:
            case AudioDeviceInfo.TYPE_DOCK:
                kind = "USB";
                break;
            case AudioDeviceInfo.TYPE_WIRED_HEADPHONES:
            case AudioDeviceInfo.TYPE_WIRED_HEADSET:
                kind = "headphones";
                break;
            default:
                kind = "audio device type " + device.getType();
        }
        CharSequence name = device.getProductName();
        return name == null || name.length() == 0 ? kind : kind + " (" + name + ")";
    }
}
