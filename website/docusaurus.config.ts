import {themes as prismThemes} from 'prism-react-renderer';
import type {Config} from '@docusaurus/types';
import type * as Preset from '@docusaurus/preset-classic';

const GITHUB_URL = 'https://github.com/KodyDennon/versionx';
const EDIT_URL = `${GITHUB_URL}/tree/main/website/`;

const config: Config = {
  title: 'Versionx',
  tagline:
    'Cross-platform, cross-language, cross-package-manager version manager and release orchestrator.',
  favicon: 'img/favicon.ico',

  future: {
    v4: true,
  },

  url: 'https://kodydennon.github.io',
  baseUrl: '/versionx/',

  organizationName: 'KodyDennon',
  projectName: 'versionx',
  deploymentBranch: 'gh-pages',
  trailingSlash: false,

  onBrokenLinks: 'throw',
  onBrokenMarkdownLinks: 'throw',

  i18n: {
    defaultLocale: 'en',
    locales: ['en'],
  },

  markdown: {
    mermaid: true,
  },

  themes: [
    '@docusaurus/theme-mermaid',
    [
      require.resolve('@easyops-cn/docusaurus-search-local'),
      {
        hashed: true,
        indexDocs: true,
        indexBlog: false,
        docsRouteBasePath: '/',
        highlightSearchTermsOnTargetPage: true,
        searchResultLimits: 10,
        explicitSearchResultPath: true,
      },
    ],
  ],

  presets: [
    [
      'classic',
      {
        docs: {
          sidebarPath: './sidebars.ts',
          routeBasePath: '/',
          editUrl: EDIT_URL,
          showLastUpdateTime: true,
        },
        blog: false,
        theme: {
          customCss: './src/css/custom.css',
        },
      } satisfies Preset.Options,
    ],
  ],

  themeConfig: {
    image: 'img/social-card.png',
    colorMode: {
      defaultMode: 'dark',
      respectPrefersColorScheme: true,
    },
    navbar: {
      title: 'Versionx',
      logo: {
        alt: 'Versionx',
        src: 'img/logo.svg',
        srcDark: 'img/logo-dark.svg',
      },
      items: [
        {
          type: 'docSidebar',
          sidebarId: 'docs',
          position: 'left',
          label: 'Docs',
        },
        {
          to: '/integrations/mcp/overview',
          label: 'MCP',
          position: 'left',
        },
        {
          to: '/sdk/overview',
          label: 'SDK',
          position: 'left',
        },
        {
          to: '/roadmap',
          label: 'Roadmap',
          position: 'left',
        },
        {
          href: GITHUB_URL,
          label: 'GitHub',
          position: 'right',
        },
      ],
    },
    footer: {
      style: 'dark',
      links: [
        {
          title: 'Docs',
          items: [
            {label: 'Get started', to: '/get-started/install'},
            {label: 'Guides', to: '/guides/managing-toolchains'},
            {label: 'Reference', to: '/reference/cli/versionx'},
            {label: 'Roadmap', to: '/roadmap'},
          ],
        },
        {
          title: 'Build with Versionx',
          items: [
            {label: 'MCP server', to: '/integrations/mcp/overview'},
            {label: 'JSON-RPC daemon', to: '/integrations/json-rpc-daemon'},
            {label: 'HTTP API', to: '/integrations/http-api'},
            {label: 'Rust SDK', to: '/sdk/overview'},
          ],
        },
        {
          title: 'Project',
          items: [
            {label: 'GitHub', href: GITHUB_URL},
            {label: 'Issues', href: `${GITHUB_URL}/issues`},
            {label: 'Discussions', href: `${GITHUB_URL}/discussions`},
            {label: 'Security', href: `${GITHUB_URL}/blob/main/SECURITY.md`},
            {label: 'License', href: `${GITHUB_URL}/blob/main/LICENSE-APACHE`},
          ],
        },
      ],
      copyright: `Copyright © ${new Date().getFullYear()} Versionx contributors. Apache-2.0 licensed. Built with Docusaurus.`,
    },
    prism: {
      theme: prismThemes.github,
      darkTheme: prismThemes.dracula,
      additionalLanguages: [
        'bash',
        'toml',
        'rust',
        'json',
        'yaml',
        'diff',
        'powershell',
        'lua',
      ],
    },
    mermaid: {
      theme: {light: 'neutral', dark: 'dark'},
    },
  } satisfies Preset.ThemeConfig,
};

export default config;
