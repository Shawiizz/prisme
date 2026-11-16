export type AudioBackend = 'wasapi' | 'asio' | 'coreAudio' | 'alsa' | 'jack' | 'aaudio' | 'usb';

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
  bufferFrames: number | null;
}

export interface AudioInfo {
  backend: AudioBackend;
  /** Name of the output device. */
  device: string;
  sampleRate: number;
  outputChannels: number;
  inputChannels: number;
  /** Decks of the engine. */
  decks: number;
  simulated: boolean;
}

/** A timecode record (or CD), one side of it when its sides differ. */
export interface TimecodeFormat {
  id: string;
  maker: string;
  /** The record, as its maker names it. */
  record: string;
  side: string | null;
  /** Where a track starts on the record, in seconds. */
  leadInS: number;
}

/** A record and its sides (one format, without a side, when they do not differ). */
export interface TimecodeRecord {
  name: string;
  sides: TimecodeFormat[];
}

/** The timecode formats by maker, then record, in the order given. */
export function timecodeMakers(
  formats: TimecodeFormat[],
): { name: string; records: TimecodeRecord[] }[] {
  const makers: { name: string; records: TimecodeRecord[] }[] = [];
  for (const f of formats) {
    let maker = makers.find((m) => m.name === f.maker);
    if (!maker) makers.push((maker = { name: f.maker, records: [] }));
    let record = maker.records.find((r) => r.name === f.record);
    if (!record) maker.records.push((record = { name: f.record, sides: [] }));
    record.sides.push(f);
  }
  return makers;
}

export const BUFFER_SIZES = [32, 64, 128, 256, 512, 1024, 2048];
