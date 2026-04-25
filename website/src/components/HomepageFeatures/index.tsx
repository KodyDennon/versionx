import type {ReactNode} from 'react';
import Heading from '@theme/Heading';
import Link from '@docusaurus/Link';
import styles from './styles.module.css';

type Feature = {
  title: string;
  href: string;
  body: ReactNode;
};

const FEATURES: Feature[] = [
  {
    title: 'Runtime & toolchain management',
    href: '/guides/managing-toolchains',
    body: (
      <>
        Pin Node, Python, Rust, Go and more per repo. Fast native shims. A drop-in
        replacement for mise / asdf that shares state with the rest of the stack.
      </>
    ),
  },
  {
    title: 'Polyglot dependency handling',
    href: '/guides/polyglot-dependency-updates',
    body: (
      <>
        Unified <code>status</code> and <code>update</code> across npm, pip, and
        cargo in the current alpha. We drive the real resolvers — we don&apos;t
        reimplement them.
      </>
    ),
  },
  {
    title: 'Release orchestration',
    href: '/guides/orchestrating-a-release',
    body: (
      <>
        SemVer, CalVer, PR-title parsing, or changesets. Plan, approve, apply, roll
        back. Cross-repo atomic releases via a saga protocol. AI-assisted changelogs
        if you want them.
      </>
    ),
  },
  {
    title: 'Policy engine with waivers',
    href: '/guides/policy-and-waivers',
    body: (
      <>
        Declarative TOML for 80% of rules, sandboxed Luau for the rest. Waivers with
        expiry. Fleet-wide enforcement for platform teams.
      </>
    ),
  },
  {
    title: 'Plan / apply, everywhere',
    href: '/sdk/plan-apply-cookbook',
    body: (
      <>
        Every mutation emits a JSON plan with Blake3-hashed prerequisites and a TTL.
        Humans approve. Agents execute. The same contract for both.
      </>
    ),
  },
  {
    title: 'AI as a client, not a component',
    href: '/integrations/mcp/overview',
    body: (
      <>
        First-class MCP server. JSON-RPC daemon. Local HTTP API. No bundled LLM — you
        bring your own key for Anthropic, OpenAI, Gemini, or Ollama.
      </>
    ),
  },
];

export default function HomepageFeatures(): ReactNode {
  return (
    <section className={styles.features}>
      <div className="container">
        <Heading as="h2" className={styles.heading}>
          What&apos;s in the box
        </Heading>
        <div className={styles.grid}>
          {FEATURES.map((f) => (
            <Link key={f.title} to={f.href} className={styles.card}>
              <h3>{f.title}</h3>
              <p>{f.body}</p>
              <span className={styles.cta}>Learn more →</span>
            </Link>
          ))}
        </div>
      </div>
    </section>
  );
}
