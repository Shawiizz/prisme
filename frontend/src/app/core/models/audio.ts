export type AudioBackend = 'wasapi' | 'asio';

export interface AudioDevice {
  name: string;
  kind: 'input' | 'output';
  channels: number;
  sampleRate: number;
  /** Periods the driver accepts (frames), when it says. */
  bufferRange: [number, number] | null;
  isDefault: boolean;
}

export interface AudioSettings {
  backend: AudioBackend;
  outputDevice: string | null;
  inputDevice: string | null;
  simulate: boolean;
  timecodeFormat: string;
  bufferFrames: number | null;
}

export interface AudioInfo {
  sampleRate: number;
  outputChannels: number;
  inputChannels: number;
  simulated: boolean;
}

export interface TimecodeFormat {
  id: string;
  name: string;
}

export const BUFFER_SIZES = [32, 64, 128, 256, 512, 1024, 2048];
