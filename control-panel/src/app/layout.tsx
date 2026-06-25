import type { Metadata } from "next";
import "./globals.css";

export const metadata: Metadata = {
  title: "远程桌面控制面板",
  description: "基于 React 19 + Next.js 15 的现代化远程控制解决方案，支持 H.264 硬件编码和 WebRTC 低延迟传输",
  keywords: ["远程控制", "桌面共享", "WebRTC", "H.264", "React 19", "Next.js"],
  authors: [{ name: "Remote Control Team" }],
  viewport: "width=device-width, initial-scale=1",
  themeColor: "#1a1a2e",
  manifest: "/manifest.json",
  icons: {
    icon: "/favicon.ico",
    apple: "/apple-touch-icon.png"
  }
};

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode;
}>) {
  return (
    <html lang="zh-CN">
      <head>
        <meta charSet="utf-8" />
        <meta name="format-detection" content="telephone=no" />
        <meta name="apple-mobile-web-app-capable" content="yes" />
        <meta name="apple-mobile-web-app-status-bar-style" content="black-translucent" />
      </head>
      <body className="font-sans antialiased">
        <div id="root">
        {children}
        </div>
      </body>
    </html>
  );
}
