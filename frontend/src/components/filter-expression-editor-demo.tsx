"use client"

import { useState } from "react"
import { FilterExpressionEditor } from "@/components/filter-expression-editor"
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card"
import { Label } from "@/components/ui/label"
import { 
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { FilterSourceType } from "@/types/api"

export function FilterExpressionEditorDemo() {
  const [expression, setExpression] = useState("")
  const [sourceType, setSourceType] = useState<FilterSourceType>("stream")

  return (
    <div className="space-y-6">
      <Card>
        <CardHeader>
          <CardTitle>Filter Expression Editor</CardTitle>
          <CardDescription>
            Rich editor for filter expressions with real-time validation and testing
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          {/* Source Type Selector */}
          <div className="space-y-2">
            <Label htmlFor="source-type">Source Type</Label>
            <Select value={sourceType} onValueChange={(value: FilterSourceType) => setSourceType(value)}>
              <SelectTrigger className="w-48">
                <SelectValue placeholder="Select source type" />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="stream">Stream Sources</SelectItem>
                <SelectItem value="epg">EPG Sources</SelectItem>
              </SelectContent>
            </Select>
          </div>

          {/* Expression Editor */}
          <div className="space-y-2">
            <Label htmlFor="expression">Filter Expression</Label>
            <FilterExpressionEditor
              value={expression}
              onChange={setExpression}
              sourceType={sourceType}
              placeholder={`Enter ${sourceType} filter expression...`}
              showTestResults={true}
              autoTest={true}
            />
          </div>

          {/* Expression Preview */}
          {expression && (
            <div className="space-y-2">
              <Label>Expression Preview</Label>
              <div className="bg-muted p-3 rounded-md">
                <code className="text-sm">{expression || "(empty)"}</code>
              </div>
            </div>
          )}
        </CardContent>
      </Card>

      {/* Example Expressions */}
      <Card>
        <CardHeader>
          <CardTitle>Example Expressions</CardTitle>
          <CardDescription>
            Click an example to try it in the editor above
          </CardDescription>
        </CardHeader>
        <CardContent>
          <div className="space-y-3">
            <div>
              <h4 className="text-sm font-medium mb-2">Basic Expressions</h4>
              <div className="space-y-2">
                {[
                  "channel_name contains 'HBO'",
                  "group_title equals 'Movies'", 
                  "tvg_id matches '^HBO.*'",
                  "not channel_name starts_with 'PPV'"
                ].map((example, i) => (
                  <button
                    key={i}
                    className="block text-left text-sm bg-muted hover:bg-muted/80 p-2 rounded w-full font-mono"
                    onClick={() => setExpression(example)}
                  >
                    {example}
                  </button>
                ))}
              </div>
            </div>

            <div>
              <h4 className="text-sm font-medium mb-2">Complex Expressions</h4>
              <div className="space-y-2">
                {[
                  "(channel_name contains 'Sport' OR group_title equals 'Sports') AND not channel_name contains 'PPV'",
                  "case_sensitive channel_name matches '^[A-Z]+.*' AND tvg_chno is_not_null",
                  "group_title in ['Movies', 'Entertainment', 'Kids'] OR channel_name starts_with 'HBO'"
                ].map((example, i) => (
                  <button
                    key={i}
                    className="block text-left text-sm bg-muted hover:bg-muted/80 p-2 rounded w-full font-mono"
                    onClick={() => setExpression(example)}
                  >
                    {example}
                  </button>
                ))}
              </div>
            </div>
          </div>
        </CardContent>
      </Card>
    </div>
  )
}