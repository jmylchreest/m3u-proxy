"use client"

import { useBackendConnectivity } from "@/providers/backend-connectivity-provider"
import { BackendUnavailable } from "@/components/backend-unavailable"
import { AppSidebar } from "@/components/app-sidebar"
import { SidebarInset, SidebarTrigger, SidebarProvider } from "@/components/ui/sidebar"
import { Separator } from "@/components/ui/separator"
import { EnhancedThemeSelector } from "@/components/enhanced-theme-selector"
import { NotificationBell } from "@/components/NotificationBell"
import { usePathname } from "next/navigation"

interface AppLayoutProps {
  children: React.ReactNode
}

function getPageTitle(pathname: string): string {
  // Remove trailing slash for consistent matching
  const normalizedPathname = pathname.endsWith('/') && pathname !== '/' ? pathname.slice(0, -1) : pathname
  
  switch (normalizedPathname) {
    case "/":
      return "Dashboard"
    case "/channels":
      return "Channel Browser"
    case "/epg":
      return "EPG Viewer"
    case "/sources/stream":
      return "Stream Sources"
    case "/sources/epg":
      return "EPG Sources"
    case "/proxies":
      return "Proxies"
    case "/admin/filters":
      return "Filters"
    case "/admin/data-mapping":
      return "Data Mapping"
    case "/admin/logos":
      return "Logos"
    case "/admin/relays":
      return "Relay Profiles"
    case "/debug":
      return "Debug"
    case "/settings":
      return "Settings"
    case "/events":
      return "Events"
    case "/logs":
      return "Logs"
    case "/color-palette":
      return "Color Palette"
    default:
      return "M3U Proxy"
  }
}

function getOperationTypeForPage(title: string): string | undefined {
  switch (title) {
    case "Stream Sources":
      return "stream_ingestion"
    case "EPG Sources":
      return "epg_ingestion"
    case "Proxies":
      return "proxy_regeneration"
    case "Events":
      return undefined // Show all operation types
    default:
      return undefined
  }
}


export function AppLayout({ children }: AppLayoutProps) {
  const { isConnected, isChecking, checkConnection, backendUrl } = useBackendConnectivity()
  const pathname = usePathname()
  const pageTitle = getPageTitle(pathname)
  const operationType = getOperationTypeForPage(pageTitle)

  // Show loading state during initial check
  if (isChecking && !isConnected) {
    return (
      <SidebarProvider>
        <AppSidebar />
        <SidebarInset>
          <main className="relative flex flex-1 flex-col bg-background">
            <header className="flex h-16 shrink-0 items-center gap-2 transition-[width,height] ease-linear group-has-data-[collapsible=icon]/sidebar-wrapper:h-12">
              <div className="flex items-center gap-2 px-4">
                <SidebarTrigger className="-ml-1" />
                <Separator orientation="vertical" className="mr-2 h-4" />
                <h1 className="text-2xl font-bold">Connecting...</h1>
              </div>
              <div className="ml-auto flex items-center gap-2 px-4">
                <NotificationBell operationType={operationType} />
                <EnhancedThemeSelector />
              </div>
            </header>
            <div className="flex flex-1 items-center justify-center">
              <div className="text-center space-y-4">
                <div className="animate-spin rounded-full h-8 w-8 border-b-2 border-primary mx-auto"></div>
                <p className="text-muted-foreground">Connecting to backend...</p>
              </div>
            </div>
          </main>
        </SidebarInset>
      </SidebarProvider>
    )
  }

  // Show backend unavailable page if not connected
  if (!isConnected) {
    return (
      <BackendUnavailable 
        onRetry={checkConnection}
        isRetrying={isChecking}
        backendUrl={backendUrl}
      />
    )
  }

  // Normal app layout when connected
  return (
    <SidebarProvider>
      <AppSidebar />
      <SidebarInset>
        <main className="relative flex flex-1 flex-col bg-background">
          <header className="flex h-16 shrink-0 items-center gap-2 transition-[width,height] ease-linear group-has-data-[collapsible=icon]/sidebar-wrapper:h-12">
            <div className="flex items-center gap-2 px-4">
              <SidebarTrigger className="-ml-1" />
              <Separator orientation="vertical" className="mr-2 h-4" />
              <h1 className="text-2xl font-bold">{pageTitle}</h1>
            </div>
            <div className="ml-auto flex items-center gap-2 px-4">
              <NotificationBell operationType={operationType} />
              <EnhancedThemeSelector />
            </div>
          </header>
          <div className="flex flex-1 flex-col gap-4 p-4 pt-0">
            {children}
          </div>
        </main>
      </SidebarInset>
    </SidebarProvider>
  )
}