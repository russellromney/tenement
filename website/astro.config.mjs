import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';

export default defineConfig({
  site: 'https://tenement.dev',
  output: 'static',
  integrations: [
    starlight({
      title: 'tenement',
      description: 'Hyperlightweight process hypervisor for single-server deployments',
      logo: {
        src: './src/assets/logo.svg',
        alt: 'tenement',
      },
      favicon: '/favicon.svg',
      components: {
        ThemeSelect: './src/components/ThemeSelect.astro',
      },
      social: {
        github: 'https://github.com/russellromney/tenement',
      },
      sidebar: [
        { label: 'Home', link: '/' },
        { label: 'Start Here', autogenerate: { directory: 'intro' } },
        { label: 'Guides', autogenerate: { directory: 'guides' } },
        { label: 'Use Cases', autogenerate: { directory: 'use-cases' } },
        { label: 'Reference', autogenerate: { directory: 'reference' } },
      ],
      customCss: ['./src/styles/custom.css'],
      editLink: {
        baseUrl: 'https://github.com/russellromney/tenement/edit/main/website/',
      },
    }),
  ],
});
