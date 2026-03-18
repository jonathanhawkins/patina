import type { Metadata } from "next";
import { Geist, Geist_Mono } from "next/font/google";
import { Navbar } from "@/components/navbar";
import "./globals.css";

const geistSans = Geist({
  variable: "--font-geist-sans",
  subsets: ["latin"],
});

const geistMono = Geist_Mono({
  variable: "--font-geist-mono",
  subsets: ["latin"],
});

export const metadata: Metadata = {
  title: "Patina Engine — Rust-Native, Godot-Compatible Game Engine",
  description:
    "A memory-safe, high-performance game engine built in Rust with full Godot scene compatibility. Open source and community driven.",
  keywords: [
    "game engine",
    "rust",
    "godot",
    "open source",
    "memory safe",
    "patina",
  ],
  openGraph: {
    title: "Patina Engine",
    description:
      "Rust-native, Godot-compatible game engine. Memory safe. High performance. Open source.",
    url: "https://patinaengine.com",
    siteName: "Patina Engine",
    type: "website",
  },
  twitter: {
    card: "summary_large_image",
    title: "Patina Engine",
    description:
      "Rust-native, Godot-compatible game engine. Memory safe. High performance. Open source.",
  },
  metadataBase: new URL("https://patinaengine.com"),
};

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode;
}>) {
  return (
    <html lang="en" className="dark">
      <body
        className={`${geistSans.variable} ${geistMono.variable} antialiased`}
      >
        <Navbar />
        {children}
      </body>
    </html>
  );
}
