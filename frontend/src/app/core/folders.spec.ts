import { folderOf, isWithin, lastName, parentOf, shortPath, stepsFrom } from './folders';

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

describe('isWithin', () => {
  it('takes the folder and what is inside it, not its namesakes', () => {
    expect(isWithin('C:\\Music', 'C:\\Music')).toBe(true);
    expect(isWithin('C:\\Music\\House', 'C:\\Music')).toBe(true);
    expect(isWithin('C:\\Musical', 'C:\\Music')).toBe(false);
    expect(isWithin('E:\\Sets', 'E:\\')).toBe(true);
  });
});

describe('folderOf', () => {
  it('is the deepest folder holding the path', () => {
    const folders = ['C:\\Music', 'C:\\Music\\Sets', 'E:\\'].map((path) => ({ path }));
    expect(folderOf(folders, 'C:\\Music\\Sets\\2025')?.path).toBe('C:\\Music\\Sets');
    expect(folderOf(folders, 'C:\\Music\\House')?.path).toBe('C:\\Music');
    expect(folderOf(folders, 'E:\\DJ')?.path).toBe('E:\\');
    expect(folderOf(folders, 'D:\\Other')).toBeNull();
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
