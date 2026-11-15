/**
 * Prisme runs on a phone or a tablet: a screen for the fingers, a controller for the mix.
 * What is named a phone in the interface serves both, upright or sideways. An iPad calls
 * itself a Mac, but is touched.
 */
export const PHONE =
  /Android|iPhone|iPad|iPod/.test(navigator.userAgent) ||
  (/Mac/.test(navigator.platform) && navigator.maxTouchPoints > 1);

/** Android lays the page out clear of its bars and its camera itself (`MainActivity.kt`). */
export const ANDROID = /Android/.test(navigator.userAgent);
