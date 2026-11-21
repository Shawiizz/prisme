import type { AudioBackend } from './backend';

/** The backends as their systems call them. */
export const BACKEND_NAMES: Record<AudioBackend, string> = {
  wasapi: 'WASAPI',
  asio: 'ASIO',
  coreAudio: 'Core Audio',
  alsa: 'ALSA',
  jack: 'JACK',
  aaudio: 'AAudio',
  usb: 'USB',
};
