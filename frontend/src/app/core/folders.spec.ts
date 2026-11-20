import { lastName, parentOf, shortPath, stepsFrom } from './folders';

describe('lastName', () => {
  it('is the last folder of a path, a drive by its letter', () => {
    expect(lastName('C:\\Users\\dj\\Music\\House')).toBe('House');
    expect(lastName('/home/dj/Music/')).toBe('Music');
    expect(lastName('E:\\')).toBe('E:');
  });
});

describe('parentOf', () => {
  it('is the folder a file is in, the top of a drive whole', () => {
    expect(parentOf('C:\\Users\\dj\\Downloads\\a.mp3')).toBe('C:\\Users\\dj\\Downloads');
    expect(parentOf('/home/dj/a.mp3')).toBe('/home/dj');
    expect(parentOf('E:\\a.mp3')).toBe('E:\\');
    expect(parentOf('/a.mp3')).toBe('/');
  });
});

describe('stepsFrom', () => {
  it('goes from the root down to the path', () => {
    expect(stepsFrom('C:\\Music', 'C:\\Music\\House\\2024')).toEqual([
      { name: 'Music', path: 'C:\\Music' },
      { name: 'House', path: 'C:\\Music\\House' },
      { name: '2024', path: 'C:\\Music\\House\\2024' },
    ]);
    expect(stepsFrom('E:\\', 'E:\\Sets')).toEqual([
      { name: 'E:', path: 'E:\\' },
      { name: 'Sets', path: 'E:\\Sets' },
    ]);
    expect(stepsFrom('/home/dj/Music', '/home/dj/Music')).toEqual([
      { name: 'Music', path: '/home/dj/Music' },
    ]);
  });
});

describe('shortPath', () => {
  it('leaves the user out of a path in their own folder', () => {
    expect(shortPath('C:\\Users\\dj\\Music')).toBe('C:\\Users\\…\\Music');
    expect(shortPath('/home/dj/Music')).toBe('~/Music');
    expect(shortPath('/Users/dj/Music')).toBe('~/Music');
    expect(shortPath('E:\\DJ')).toBe('E:\\DJ');
  });
});
