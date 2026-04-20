import type {ReactNode} from 'react';
import clsx from 'clsx';
import Link from '@docusaurus/Link';
import useDocusaurusContext from '@docusaurus/useDocusaurusContext';
import Layout from '@theme/Layout';
import Heading from '@theme/Heading';
import CodeBlock from '@theme/CodeBlock';

import HomepageFeatures from '@site/src/components/HomepageFeatures';

import styles from './index.module.css';

const INSTALL_SNIPPET = `# macOS / Linux
curl --proto '=https' --tlsv1.2 -LsSf \\
  https://github.com/KodyDennon/versionx/releases/latest/download/versionx-cli-installer.sh | sh

# Windows (PowerShell)
powershell -ExecutionPolicy ByPass -c "irm https://github.com/KodyDennon/versionx/releases/latest/download/versionx-cli-installer.ps1 | iex"

# From source
cargo install versionx-cli`;

const DEMO_SNIPPET = `$ versionx
Versionx 0.7.0

Workspace  ./my-app    (node 22.11.0, python 3.13.1, rust 1.95)
Outdated   3 packages in apps/web  (axios ^1.6 → ^1.7)
Policy     clean
Ready      release plan   (last release 12d ago)

What next?
  versionx status                show ecosystem + release health
  versionx update --plan         preview dependency bumps
  versionx release plan          propose the next release`;

function HomepageHeader(): ReactNode {
  const {siteConfig} = useDocusaurusContext();
  return (
    <header className={styles.hero}>
      <div className="container">
        <div className={styles.heroInner}>
          <div className={styles.heroCopy}>
            <Heading as="h1" className={styles.heroTitle}>
              One tool for runtimes, dependencies, and releases.
            </Heading>
            <p className={styles.heroSubtitle}>{siteConfig.tagline}</p>
            <p className={styles.heroPitch}>
              Versionx unifies toolchain management, polyglot dependency handling,
              SemVer release orchestration, multi-repo coordination, policy, and
              first-class AI-agent integration — behind one progressive-disclosure CLI.
            </p>
            <div className={styles.buttons}>
              <Link
                className="button button--primary button--lg"
                to="/get-started/install">
                Install →
              </Link>
              <Link
                className="button button--secondary button--lg"
                to="/introduction/what-is-versionx">
                Why Versionx
              </Link>
              <Link
                className={clsx('button button--outline button--lg', styles.buttonQuiet)}
                to="https://github.com/KodyDennon/versionx">
                GitHub
              </Link>
            </div>
            <p className={styles.heroStatus}>
              <strong>0.7 feature-complete.</strong> 30 crates, 280+ tests.{' '}
              <Link to="/roadmap">Road to 1.0 →</Link>
            </p>
          </div>
          <div className={styles.heroDemo}>
            <CodeBlock language="console" title="versionx">
              {DEMO_SNIPPET}
            </CodeBlock>
          </div>
        </div>
      </div>
    </header>
  );
}

function InstallBand(): ReactNode {
  return (
    <section className={styles.installBand}>
      <div className="container">
        <Heading as="h2" className={styles.sectionTitle}>
          Install
        </Heading>
        <p className={styles.sectionLede}>
          One static binary. Linux, macOS, Windows (x86_64 and aarch64). No runtime
          dependencies except git and the ecosystem tools Versionx drives.
        </p>
        <CodeBlock language="bash">{INSTALL_SNIPPET}</CodeBlock>
        <p className={styles.sectionFoot}>
          Homebrew, Scoop, Cargo, npm and PyPI shims — see{' '}
          <Link to="/get-started/install">Install</Link> for every platform.
        </p>
      </div>
    </section>
  );
}

function AudienceCards(): ReactNode {
  return (
    <section className={styles.audienceBand}>
      <div className="container">
        <div className={styles.audienceGrid}>
          <Link className={styles.audienceCard} to="/get-started/quickstart">
            <h3>Run it on your repo</h3>
            <p>
              Zero-config. Bare <code>versionx</code> detects your ecosystems,
              suggests next steps, and stays out of your way.
            </p>
            <span className={styles.cta}>Quickstart →</span>
          </Link>
          <Link className={styles.audienceCard} to="/integrations/mcp/overview">
            <h3>Drive it from an agent</h3>
            <p>
              MCP server + JSON-RPC daemon + HTTP API. Every mutation is
              plan/apply with Blake3-hashed prerequisites.
            </p>
            <span className={styles.cta}>Integrations →</span>
          </Link>
          <Link className={styles.audienceCard} to="/contributing/dev-environment">
            <h3>Contribute to Versionx</h3>
            <p>
              A 30-crate Rust workspace. <code>cargo xtask ci</code> runs
              everything CI does. 280+ tests travel with the code.
            </p>
            <span className={styles.cta}>Contributing →</span>
          </Link>
        </div>
      </div>
    </section>
  );
}

export default function Home(): ReactNode {
  const {siteConfig} = useDocusaurusContext();
  return (
    <Layout
      title={siteConfig.title}
      description={siteConfig.tagline}>
      <HomepageHeader />
      <main>
        <HomepageFeatures />
        <InstallBand />
        <AudienceCards />
      </main>
    </Layout>
  );
}
