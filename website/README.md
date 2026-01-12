# tenement.dev Website

This is the official website for [tenement](https://tenement.dev), built with [Astro](https://astro.build) and [Starlight](https://starlight.astro.build).

## Development

### Prerequisites

- Node.js 18+ with npm

### Installation

```bash
npm install
```

### Development Server

```bash
npm run dev
```

Site will be available at `http://localhost:3000`

### Build

```bash
npm run build
```

Output is in `dist/`

### Preview

```bash
npm run preview
```

## Structure

```
src/
├── content/
│   └── docs/
│       ├── intro/           # Getting started pages
│       ├── guides/          # How-to guides
│       ├── use-cases/       # Real-world examples
│       └── reference/       # API, CLI, roadmap
├── styles/
│   └── custom.css          # Custom styling
└── assets/                  # Images, logos, etc.
```

## Content

All documentation is in Markdown format under `src/content/docs/`. Files are automatically organized into the sidebar based on directory structure (configured in `astro.config.mjs`).

### Adding Pages

1. Create a new `.md` file in the appropriate directory
2. Add frontmatter (title, description)
3. Write markdown content
4. File appears in sidebar automatically

Example:

```markdown
---
title: My New Guide
description: A guide about something useful
---

# My New Guide

Content goes here...
```

## Styling

Custom styles are in `src/styles/custom.css`. The site uses CSS custom properties (variables) for theming.

Key colors:
- **Primary**: `--color-primary` (Teal #2dd4bf)
- **Background**: `--color-bg-dark` (Slate #0f172a)
- **Text**: `--color-text-primary` (Light slate #f1f5f9)

## Deployment

### Vercel (Recommended)

```bash
npm install -g vercel
vercel
```

### GitHub Pages

Configure in `astro.config.mjs`:

```javascript
export default defineConfig({
  site: 'https://yourusername.github.io/tenement',
  // ...
});
```

Then deploy:

```bash
npm run build
git add dist/
git commit -m "Deploy"
git push
```

### Manual

Build and upload `dist/` to any static host (Netlify, Cloudflare Pages, etc.)

## Configuration

### Site Metadata

Edit `astro.config.mjs`:
- `site` - Your domain
- `title` - Site title
- `description` - Meta description
- `logo` - Logo path
- `favicon` - Favicon path

### Sidebar Navigation

Edit the `sidebar` array in `astro.config.mjs` to add/remove sections and pages.

## License

Apache 2.0 - See [LICENSE](../LICENSE)
