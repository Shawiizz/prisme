// @ts-check
import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';

const group = (label, fr, directory) => ({
	label,
	translations: { fr },
	collapsed: true,
	items: [{ autogenerate: { directory: `docs/${directory}` } }],
});

export default defineConfig({
	site: 'https://prisme.maelferi.com',
	devToolbar: { enabled: false },
	integrations: [
		starlight({
			title: 'Prisme',
			description: 'DJ software built for vinyl control: timecode records, stems, controllers.',
			logo: { src: './src/assets/logo-mark.svg', alt: 'Prisme' },
			favicon: '/favicon.svg',
			defaultLocale: 'root',
			locales: {
				root: { label: 'English', lang: 'en' },
				fr: { label: 'Français', lang: 'fr' },
			},
			social: [{ icon: 'github', label: 'GitHub', href: 'https://github.com/Shawiizz/prisme' }],
			editLink: { baseUrl: 'https://github.com/Shawiizz/prisme/edit/main/website/' },
			customCss: ['./src/styles/fonts.css', './src/styles/theme.css'],
			components: {
				ThemeProvider: './src/components/ThemeProvider.astro',
				ThemeSelect: './src/components/ThemeSelect.astro',
			},
			sidebar: [
				{ label: 'Introduction', translations: { fr: 'Introduction' }, slug: 'docs' },
				group('Getting started', 'Premiers pas', 'start'),
				group('Interface', 'Interface', 'interface'),
				group('Library', 'Bibliothèque', 'library'),
				group('Decks', 'Decks', 'decks'),
				group('Mixer', 'Table de mixage', 'mixer'),
				group('Effects and stems', 'Effets et stems', 'sound'),
				group('Vinyl control', 'Contrôle vinyle', 'dvs'),
				group('Controllers', 'Contrôleurs', 'controllers'),
				group('Settings', 'Réglages', 'settings'),
				group('Help', 'Aide', 'help'),
				group('Developers', 'Développeurs', 'develop'),
			],
		}),
	],
});
