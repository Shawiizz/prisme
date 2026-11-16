export const REPO = 'https://github.com/Shawiizz/prisme';
export const RELEASES = `${REPO}/releases/latest`;

export interface Release {
	version: string | null;
	windows: string;
	macos: string;
	deb: string;
	appImage: string;
}

let cached: Promise<Release> | undefined;

/** The latest release's packages, read once per build; the releases page when GitHub does not answer. */
export function latestRelease(): Promise<Release> {
	cached ??= fetchRelease();
	return cached;
}

async function fetchRelease(): Promise<Release> {
	const fallback: Release = { version: null, windows: RELEASES, macos: RELEASES, deb: RELEASES, appImage: RELEASES };
	try {
		const response = await fetch('https://api.github.com/repos/Shawiizz/prisme/releases/latest', {
			// in CI the workflow's token lifts the limit GitHub puts on anonymous calls
			headers: {
				Accept: 'application/vnd.github+json',
				...(process.env.GITHUB_TOKEN ? { Authorization: `Bearer ${process.env.GITHUB_TOKEN}` } : {}),
			},
			signal: AbortSignal.timeout(5000),
		});
		if (!response.ok) return fallback;
		const release = (await response.json()) as {
			tag_name: string;
			assets: { name: string; browser_download_url: string }[];
		};
		const asset = (suffix: string) =>
			release.assets.find((a) => a.name.endsWith(suffix))?.browser_download_url ?? RELEASES;
		return {
			version: release.tag_name,
			windows: asset('-setup.exe'),
			macos: asset('.dmg'),
			deb: asset('.deb'),
			appImage: asset('.AppImage'),
		};
	} catch {
		return fallback;
	}
}
