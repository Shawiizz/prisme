/** Positions closer than this are the same (on a 0-1 scale). */
const NEAR = 0.02;

/**
 * Soft takeover: a controller's fader whose position no longer matches its setting (moved
 * on the screen meanwhile) takes it over only once it reaches it, instead of making it
 * jump. The same rule as `prisme_control::Pickup`, for the faders the UI performs.
 */
export class Pickup {
  /** The fader's previous position, to see it cross the setting. */
  private previous: number | null = null;
  /** The setting the fader last gave, to notice it changed elsewhere. */
  private given: number | null = null;

  /** Whether the fader at `position` drives a setting now at `current` (both 0-1). */
  takes(position: number, current: number): boolean {
    const held = this.given !== null && Math.abs(this.given - current) <= NEAR / 4;
    const crossed = this.previous !== null && (this.previous - current) * (position - current) <= 0;
    const takes = held || crossed || Math.abs(position - current) <= NEAR;
    this.previous = position;
    this.given = takes ? position : null;
    return takes;
  }
}
