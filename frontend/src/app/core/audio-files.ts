/** The extensions of the files `prisme_media::decode_file` reads. */
export const AUDIO_EXTENSIONS = [
  'mp3',
  'flac',
  'wav',
  'aif',
  'aiff',
  'm4a',
  'aac',
  'mp4',
  'ogg',
  'oga',
];

/** Whether `path` names a file Prisme plays, by its extension. */
export function isAudioFile(path: string): boolean {
  const extension = /\.([^.\/]+)$/.exec(path)?.[1].toLowerCase();
  return extension !== undefined && AUDIO_EXTENSIONS.includes(extension);
}
