/**
 * Prisme runs on a phone or a tablet: a screen for the fingers, a controller for the mix.
 * An iPad calls itself a Mac, but is touched.
 */
export const PHONE =
  /Android|iPhone|iPad|iPod/.test(navigator.userAgent) ||
  (/Mac/.test(navigator.platform) && navigator.maxTouchPoints > 1);
