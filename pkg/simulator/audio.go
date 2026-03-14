package simulator

import (
	"github.com/jurgelenas/edgetx-cli/pkg/logging"
	"github.com/veandco/go-sdl2/sdl"
)

const (
	audioSampleRate = 32000
	audioChannels   = 1
	audioSamples    = 1024
	audioFormat     = sdl.AUDIO_S16LSB
)

// AudioPlayer handles PCM audio playback via SDL2.
type AudioPlayer struct {
	device sdl.AudioDeviceID
	queue  <-chan []int16
	volume int
	stop   chan struct{}
}

// NewAudioPlayer creates an audio player reading from the given channel.
func NewAudioPlayer(queue <-chan []int16) (*AudioPlayer, error) {
	spec := &sdl.AudioSpec{
		Freq:     audioSampleRate,
		Format:   audioFormat,
		Channels: audioChannels,
		Samples:  audioSamples,
	}

	var obtained sdl.AudioSpec
	deviceID, err := sdl.OpenAudioDevice("", false, spec, &obtained, 0)
	if err != nil {
		return nil, err
	}

	ap := &AudioPlayer{
		device: deviceID,
		queue:  queue,
		volume: sdl.MIX_MAXVOLUME,
		stop:   make(chan struct{}),
	}

	// Unpause the device to start playing.
	sdl.PauseAudioDevice(ap.device, false)

	// Start the audio pump goroutine.
	go ap.pump()

	return ap, nil
}

func (ap *AudioPlayer) pump() {
	for {
		select {
		case samples, ok := <-ap.queue:
			if !ok {
				return
			}
			ap.queueSamples(samples)
		case <-ap.stop:
			return
		}
	}
}

func (ap *AudioPlayer) queueSamples(samples []int16) {
	// Convert []int16 to []byte for SDL.
	buf := make([]byte, len(samples)*2)
	for i, s := range samples {
		buf[i*2] = byte(s)
		buf[i*2+1] = byte(s >> 8)
	}

	if err := sdl.QueueAudio(ap.device, buf); err != nil {
		logging.WithError(err).Debug("failed to queue audio")
	}
}

// SetVolume sets the audio volume (0-128, where 128 = SDL_MIX_MAXVOLUME).
func (ap *AudioPlayer) SetVolume(vol int) {
	if vol < 0 {
		vol = 0
	}
	if vol > sdl.MIX_MAXVOLUME {
		vol = sdl.MIX_MAXVOLUME
	}
	ap.volume = vol
}

// Close stops playback and releases the audio device.
func (ap *AudioPlayer) Close() {
	close(ap.stop)
	sdl.PauseAudioDevice(ap.device, true)
	sdl.CloseAudioDevice(ap.device)
}
