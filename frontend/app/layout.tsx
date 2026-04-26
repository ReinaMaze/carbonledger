import type { Metadata } from "next";

export const metadata: Metadata = {
  title: "CarbonLedger — Verified Carbon Credits on Stellar",
  description: "Buy and retire verified carbon credits with full on-chain provenance. Every retirement is permanent and publicly verifiable.",
};

const NAV_LINKS = [
  { href: "/marketplace", label: "Marketplace" },
  { href: "/projects",    label: "Projects" },
  { href: "/audit",       label: "Audit Trail" },
  { href: "/dashboard",   label: "Dashboard" },
];

export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="en">
      <head>
        <link rel="preconnect" href="https://fonts.googleapis.com" />
        <link rel="preconnect" href="https://fonts.gstatic.com" crossOrigin="anonymous" />
        <link href="https://fonts.googleapis.com/css2?family=Inter:wght@400;500;600;700;800&display=swap" rel="stylesheet" />
        <style>{`
          :focus-visible {
            outline: 3px solid #16a34a;
            outline-offset: 2px;
            border-radius: 2px;
          }
          .skip-nav {
            position: absolute;
            top: -100%;
            left: 1rem;
            z-index: 9999;
            background: #16a34a;
            color: #fff;
            padding: 0.5rem 1rem;
            border-radius: 0 0 0.375rem 0.375rem;
            font-weight: 600;
            font-size: 0.875rem;
            text-decoration: none;
          }
          .skip-nav:focus {
            top: 0;
          }
        `}</style>
      </head>
      <body style={{ margin: 0, fontFamily: "'Inter', sans-serif", background: "#f9fafb", color: "#111827" }}>
        <a href="#main-content" className="skip-nav">Skip to main content</a>
        <nav
          role="navigation"
          aria-label="Main navigation"
          style={{
            background: "#fff",
            borderBottom: "1px solid #e5e7eb",
            padding: "0 2rem",
            height: "64px",
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
            position: "sticky",
            top: 0,
            zIndex: 100,
          }}
        >
          <a href="/" aria-label="CarbonLedger home" style={{ display: "flex", alignItems: "center", gap: "0.5rem", textDecoration: "none" }}>
            <span aria-hidden="true" style={{ fontSize: "1.5rem" }}>🌿</span>
            <span style={{ fontWeight: 800, fontSize: "1.1rem", color: "#15803d" }}>CarbonLedger</span>
          </a>
          <div style={{ display: "flex", gap: "1.5rem", alignItems: "center" }}>
            {NAV_LINKS.map(({ href, label }) => (
              <a
                key={href}
                href={href}
                style={{ fontSize: "0.875rem", fontWeight: 500, color: "#374151", textDecoration: "none" }}
              >
                {label}
              </a>
            ))}
            <a
              href="/buy"
              style={{
                background: "#16a34a",
                color: "#fff",
                borderRadius: "0.5rem",
                padding: "0.5rem 1rem",
                fontSize: "0.875rem",
                fontWeight: 600,
                textDecoration: "none",
              }}
            >
              Buy Credits
            </a>
          </div>
        </nav>
        <main id="main-content" tabIndex={-1}>{children}</main>
      </body>
    </html>
  );
}
