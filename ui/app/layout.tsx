import type { Metadata } from "next";
import Appearance from "../components/Appearance";
import "../styles/fonts.css";
import "../styles/theme.css";
import "../styles/atoms.css";
import "../styles/app.css";
export const metadata: Metadata = { title: "Motetto", description: "Local meeting notes" };
export default function Layout({children}:Readonly<{children:React.ReactNode}>) {
  return <html lang="en" suppressHydrationWarning><head><script src="/appearance.js"/></head><body><Appearance/>{children}</body></html>;
}
