import type { Metadata } from "next";
import { Instrument_Sans, JetBrains_Mono } from "next/font/google";
import { Navbar } from "@/components/navbar";
import "./globals.css";

const instrumentSans = Instrument_Sans({
  variable: "--font-sans",
  subsets: ["latin"],
  display: "swap",
});

const jetbrainsMono = JetBrains_Mono({
  variable: "--font-mono",
  subsets: ["latin"],
  display: "swap",
});

export const metadata: Metadata = {
  title: {
    default: "Patina Engine — Rust-Native, Godot-Compatible Game Engine",
    template: "%s — Patina Engine",
  },
  description:
    "A memory-safe, high-performance game engine built in Rust with full Godot scene compatibility. Open source and community driven.",
  keywords: [
    "game engine",
    "rust",
    "godot",
    "open source",
    "memory safe",
    "patina",
    "game development",
    "rust game engine",
  ],
  openGraph: {
    title: "Patina Engine — Rust-Native, Godot-Compatible Game Engine",
    description:
      "A memory-safe, high-performance game engine built in Rust with full Godot scene compatibility. Open source and community driven.",
    url: "https://patinaengine.com",
    siteName: "Patina Engine",
    type: "website",
    images: [
      {
        url: "https://patinaengine.com/og-image.png",
        width: 1200,
        height: 630,
        alt: "Patina Engine — Rust-Native, Godot-Compatible Game Engine",
      },
    ],
  },
  twitter: {
    card: "summary_large_image",
    title: "Patina Engine — Rust-Native, Godot-Compatible Game Engine",
    description:
      "A memory-safe, high-performance game engine built in Rust with full Godot scene compatibility. Open source and community driven.",
    images: ["https://patinaengine.com/og-image.png"],
  },
  metadataBase: new URL("https://patinaengine.com"),
  robots: {
    index: true,
    follow: true,
  },
};

const jsonLd = {
  "@context": "https://schema.org",
  "@type": "SoftwareApplication",
  name: "Patina Engine",
  description:
    "A memory-safe, high-performance game engine built in Rust with full Godot scene compatibility.",
  applicationCategory: "DeveloperApplication",
  operatingSystem: "Windows, macOS, Linux",
  url: "https://patinaengine.com",
  license: "https://opensource.org/licenses/MIT",
  offers: {
    "@type": "Offer",
    price: "0",
    priceCurrency: "USD",
  },
  author: {
    "@type": "Organization",
    name: "Patina Engine",
    url: "https://patinaengine.com",
  },
};

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode;
}>) {
  return (
    <html lang="en" className="dark">
      <head>
        <script
          type="application/ld+json"
          dangerouslySetInnerHTML={{ __html: JSON.stringify(jsonLd) }}
        />
      </head>
      <body
        className={`${instrumentSans.variable} ${jetbrainsMono.variable} antialiased`}
      >
        {/* Subtle noise texture overlay for depth */}
        <div
          className="pointer-events-none fixed inset-0 z-50 opacity-[0.03]"
          style={{
            backgroundImage: `url("data:image/svg+xml,%3Csvg viewBox='0 0 256 256' xmlns='http://www.w3.org/2000/svg'%3E%3Cfilter id='n'%3E%3CfeTurbulence type='fractalNoise' baseFrequency='0.9' numOctaves='4' stitchTiles='stitch'/%3E%3C/filter%3E%3Crect width='100%25' height='100%25' filter='url(%23n)'/%3E%3C/svg%3E")`,
            backgroundRepeat: "repeat",
            backgroundSize: "256px 256px",
          }}
        />
        <Navbar />
        {children}
      </body>
    </html>
  );
}
