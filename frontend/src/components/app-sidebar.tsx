"use client"

import {
  Play,
  Filter,
  Image,
  Activity,
  Server,
  Database,
  Zap,
  Palette,
  Bug,
  FileText,
  Settings,
  Radio,
  ArrowUpDown
} from "lucide-react"
import Link from "next/link"
import { usePathname } from "next/navigation"
import { cn } from "@/lib/utils"
import {
  Sidebar,
  SidebarHeader,
  SidebarContent,
  SidebarGroup,
  SidebarGroupLabel,
  SidebarGroupContent,
  SidebarMenu,
  SidebarMenuItem,
  SidebarMenuButton,
} from "@/components/ui/sidebar"

const navigation = [
  {
    title: "Overview",
    items: [
      {
        title: "Dashboard",
        url: "/",
        icon: Activity,
      },
    ],
  },
  {
    title: "Proxy Config",
    items: [
      {
        title: "Stream Sources",
        url: "/sources/stream",
        icon: Database,
      },
      {
        title: "EPG Sources",
        url: "/sources/epg",
        icon: Server,
      },
      {
        title: "Proxies",
        url: "/proxies",
        icon: Play,
      },
    ],
  },
  {
    title: "Global Config",
    items: [
      {
        title: "Filters",
        url: "/admin/filters",
        icon: Filter,
      },
      {
        title: "Data Mapping",
        url: "/admin/data-mapping",
        icon: ArrowUpDown,
      },
      {
        title: "Logos",
        url: "/admin/logos",
        icon: Image,
      },
      {
        title: "Relay Profiles",
        url: "/admin/relays",
        icon: Zap,
      },
    ],
  },
  {
    title: "Debug",
    items: [
      {
        title: "Debug",
        url: "/debug",
        icon: Bug,
      },
      {
        title: "Settings",
        url: "/settings",
        icon: Settings,
      },
      {
        title: "Events",
        url: "/events",
        icon: Activity,
      },
      {
        title: "Logs",
        url: "/logs",
        icon: FileText,
      },
      {
        title: "Color Palette",
        url: "/color-palette",
        icon: Palette,
      },
    ],
  },
]

export function AppSidebar() {
  const pathname = usePathname()

  return (
    <Sidebar variant="inset" collapsible="icon">
      <SidebarHeader>
        <div className="flex items-center gap-2 px-2 py-2">
          <Radio className="h-4 w-4" />
          <div className="group-data-[collapsible=icon]:hidden">
            <h2 className="text-lg font-semibold">M3U Proxy</h2>
          </div>
        </div>
      </SidebarHeader>

      <SidebarContent>
        {navigation.map((group) => (
          <SidebarGroup key={group.title}>
            <SidebarGroupLabel>{group.title}</SidebarGroupLabel>
            <SidebarGroupContent>
              <SidebarMenu>
                {group.items.map((item) => (
                  <SidebarMenuItem key={item.title}>
                    <SidebarMenuButton 
                      asChild
                      isActive={pathname === item.url}
                      tooltip={item.title}
                    >
                      <Link href={item.url}>
                        <item.icon />
                        <span>{item.title}</span>
                      </Link>
                    </SidebarMenuButton>
                  </SidebarMenuItem>
                ))}
              </SidebarMenu>
            </SidebarGroupContent>
          </SidebarGroup>
        ))}
      </SidebarContent>

    </Sidebar>
  )
}