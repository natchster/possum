package io.github.natchster.possum;

import android.media.AudioAttributes;
import android.media.AudioFormat;
import android.media.AudioTrack;
import android.util.Log;

/** The screams, ready to play: one preloaded AudioTrack per variation. */
final class Voices {
    /** As loud as the desktop app's default. */
    private static final float VOLUME = 0.6f;

    private final AudioTrack[][] tracks = new AudioTrack[2][];

    Voices() {
        AudioAttributes attributes = new AudioAttributes.Builder()
                .setUsage(AudioAttributes.USAGE_MEDIA)
                .setContentType(AudioAttributes.CONTENT_TYPE_SONIFICATION)
                .build();
        AudioFormat format = new AudioFormat.Builder()
                .setSampleRate(Native.sampleRate())
                .setEncoding(AudioFormat.ENCODING_PCM_16BIT)
                .setChannelMask(AudioFormat.CHANNEL_OUT_MONO)
                .build();
        for (int kind = 0; kind < tracks.length; kind++) {
            tracks[kind] = new AudioTrack[Native.voiceCount(kind)];
            for (int variant = 0; variant < tracks[kind].length; variant++) {
                short[] samples = Native.voice(kind, variant);
                if (samples == null) continue;
                try {
                    AudioTrack track = new AudioTrack.Builder()
                            .setAudioAttributes(attributes)
                            .setAudioFormat(format)
                            .setTransferMode(AudioTrack.MODE_STATIC)
                            .setBufferSizeInBytes(samples.length * 2)
                            .build();
                    track.write(samples, 0, samples.length);
                    track.setVolume(VOLUME);
                    tracks[kind][variant] = track;
                } catch (RuntimeException e) {
                    Log.w(PossumService.TAG, "can't prepare a scream", e);
                }
            }
        }
    }

    /** Plays a scream from the start; false if it can't. */
    boolean play(int kind, int variant) {
        if (kind < 0 || kind >= tracks.length || variant < 0 || variant >= tracks[kind].length) return false;
        AudioTrack track = tracks[kind][variant];
        if (track == null) return false;
        try {
            track.pause();
            track.reloadStaticData();
            track.play();
            return true;
        } catch (IllegalStateException e) {
            Log.w(PossumService.TAG, "can't play a scream", e);
            return false;
        }
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
                if (track != null) track.release();
            }
        }
    }
}
