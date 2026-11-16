/** Long enough for the word to come fully out of the prism. */
const SHOWN_AT_LEAST_MS = 1600;

/** Fades out the splash of index.html once the app is ready, never before its animation ends. */
export async function dismissSplash(): Promise<void> {
  const splash = document.getElementById('splash');
  if (!splash) return;
  const left = SHOWN_AT_LEAST_MS - performance.now();
  if (left > 0) await new Promise((done) => setTimeout(done, left));
  splash.addEventListener('transitionend', () => splash.remove(), { once: true });
  splash.classList.add('gone');
}
