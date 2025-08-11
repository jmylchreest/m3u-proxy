import { Logs } from "@/components/logs"
import { AppHeader } from "@/components/app-header"

export default function LogsPage() {
  return (
    <>
      <AppHeader title="Logs" />
      <div className="flex-1 p-6">
        <Logs />
      </div>
    </>
  )
}