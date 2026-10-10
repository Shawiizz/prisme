export interface AudioDevice {
  name: string;
  kind: 'input' | 'output';
  channels: number;
  sampleRate: number;
  isDefault: boolean;
}

export interface AudioSettings {
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
