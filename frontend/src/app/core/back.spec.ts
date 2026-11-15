import { BackService } from './back';

describe('BackService', () => {
  let go: ReturnType<typeof vi.spyOn>;

  beforeEach(() => {
    go = vi.spyOn(history, 'go').mockImplementation(() => {});
    vi.spyOn(history, 'pushState').mockImplementation(() => {});
  });

  afterEach(() => vi.restoreAllMocks());

  it('walks back once for what is closed together', async () => {
    const back = new BackService();
    const first = back.opened(() => {});
    const second = back.opened(() => {});
    first();
    second();
    await Promise.resolve();
    expect(go).toHaveBeenCalledOnce();
    expect(go).toHaveBeenCalledWith(-2);
  });

  it('leaves the walk back of its own to the gesture after it', async () => {
    const back = new BackService();
    const closed: string[] = [];
    back.opened(() => closed.push('under'));
    back.opened(() => closed.push('over'))();
    await Promise.resolve();
    dispatchEvent(new PopStateEvent('popstate'));
    expect(closed).toEqual([]);
    dispatchEvent(new PopStateEvent('popstate'));
    expect(closed).toEqual(['under']);
  });
});
