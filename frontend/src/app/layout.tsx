import type { Metadata } from "next"
import "./globals.css"
import { EnhancedThemeProvider } from "@/components/enhanced-theme-provider"
import { BackendConnectivityProvider } from "@/providers/backend-connectivity-provider"
import { ProgressProvider } from "@/providers/ProgressProvider"
import { FeatureFlagsProvider } from "@/providers/FeatureFlagsProvider"
import { AppLayout } from "@/components/app-layout"
import { enhancedThemeScript } from "@/lib/enhanced-theme-script"

export const metadata: Metadata = {
  title: "M3U Proxy UI",
  description: "Modern web interface for M3U Proxy service",
}

export default function RootLayout({
  children,
}: {
  children: React.ReactNode
}) {
  return (
    <html lang="en" suppressHydrationWarning>
      <body>
        <script dangerouslySetInnerHTML={{ __html: enhancedThemeScript }} />
        <EnhancedThemeProvider defaultTheme="graphite" defaultMode="system">
          <BackendConnectivityProvider>
            <FeatureFlagsProvider>
              <ProgressProvider>
                <AppLayout>
                  {children}
                </AppLayout>
              </ProgressProvider>
            </FeatureFlagsProvider>
          </BackendConnectivityProvider>
        </EnhancedThemeProvider>
      </body>
    </html>
  )
}