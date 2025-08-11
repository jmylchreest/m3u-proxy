"use client"

import { useState, useRef, useEffect, useCallback } from "react"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { Button } from "@/components/ui/button"
import { Badge } from "@/components/ui/badge"
import { Alert, AlertDescription } from "@/components/ui/alert"
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip"
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover"
import {
  CheckCircle,
  XCircle,
  Lightbulb,
  Code2,
  Zap,
  Hash,
  Type,
  Settings,
  HelpCircle,
  WandIcon,
  ChevronRight
} from "lucide-react"
import { apiClient } from "@/lib/api-client"

interface FilterSyntaxEditorProps {
  value: string
  onChange: (value: string) => void
  placeholder?: string
  disabled?: boolean
  className?: string
}

interface SyntaxToken {
  type: 'field' | 'operator' | 'value' | 'logic' | 'modifier' | 'error' | 'whitespace' | 'parenthesis'
  value: string
  start: number
  end: number
  valid?: boolean
  suggestion?: string
}

interface ValidationResult {
  valid: boolean
  error?: string
  match_count?: number
  tokens?: SyntaxToken[]
}

const KEYWORDS = {
  operators: ['contains', 'not_contains', 'equals', 'not_equals', 'matches', 'not_matches', 'starts_with', 'ends_with'],
  logic: ['AND', 'OR', 'and', 'or'],
  modifiers: ['not', 'case_sensitive']
}

const COMMON_FIELDS = ['channel_name', 'group_title', 'tvg_id', 'tvg_logo', 'tvg_country', 'stream_url']

const SYNTAX_EXAMPLES = [
  'channel_name contains "sport"',
  'group_title equals "News" AND channel_name not_contains "adult"',
  'channel_name matches ".*HD.*" OR channel_name matches ".*4K.*"',
  '(channel_name contains "BBC" OR channel_name contains "CNN") AND group_title not_contains "Adult"',
  'not channel_name contains "test"',
  'channel_name case_sensitive contains "BBC"'
]

function tokenizeExpression(expression: string, availableFields: string[]): SyntaxToken[] {
  const tokens: SyntaxToken[] = []
  const regex = /("[^"]*")|(\w+)|([()&|])|(\s+)/g
  let match

  while ((match = regex.exec(expression)) !== null) {
    const value = match[0]
    const start = match.index
    const end = start + value.length

    if (match[1]) { // Quoted string
      tokens.push({
        type: 'value',
        value,
        start,
        end,
        valid: true
      })
    } else if (match[2]) { // Word
      const word = value.toLowerCase()
      let type: SyntaxToken['type'] = 'error'
      let valid = false

      if (availableFields.includes(value) || COMMON_FIELDS.includes(value)) {
        type = 'field'
        valid = true
      } else if (KEYWORDS.operators.some(op => op.toLowerCase() === word)) {
        type = 'operator'
        valid = true
      } else if (KEYWORDS.logic.some(logic => logic.toLowerCase() === word)) {
        type = 'logic'
        valid = true
      } else if (KEYWORDS.modifiers.some(mod => mod.toLowerCase() === word)) {
        type = 'modifier'
        valid = true
      }

      tokens.push({
        type,
        value,
        start,
        end,
        valid,
        suggestion: !valid ? findClosestMatch(word, [...KEYWORDS.operators, ...KEYWORDS.logic, ...KEYWORDS.modifiers, ...availableFields]) : undefined
      })
    } else if (match[3]) { // Parentheses, operators
      tokens.push({
        type: 'parenthesis',
        value,
        start,
        end,
        valid: true
      })
    } else if (match[4]) { // Whitespace
      tokens.push({
        type: 'whitespace',
        value,
        start,
        end,
        valid: true
      })
    }
  }

  return tokens
}

function findClosestMatch(word: string, candidates: string[]): string | undefined {
  const lowerWord = word.toLowerCase()
  
  // Exact match
  const exact = candidates.find(c => c.toLowerCase() === lowerWord)
  if (exact) return exact

  // Starts with
  const startsWith = candidates.find(c => c.toLowerCase().startsWith(lowerWord))
  if (startsWith) return startsWith

  // Contains
  const contains = candidates.find(c => c.toLowerCase().includes(lowerWord))
  if (contains) return contains

  // Levenshtein distance
  let closest = candidates[0]
  let minDistance = levenshteinDistance(lowerWord, closest.toLowerCase())

  for (const candidate of candidates.slice(1)) {
    const distance = levenshteinDistance(lowerWord, candidate.toLowerCase())
    if (distance < minDistance) {
      minDistance = distance
      closest = candidate
    }
  }

  return minDistance <= 2 ? closest : undefined
}

function levenshteinDistance(a: string, b: string): number {
  const matrix = Array(b.length + 1).fill(null).map(() => Array(a.length + 1).fill(null))

  for (let i = 0; i <= a.length; i++) matrix[0][i] = i
  for (let j = 0; j <= b.length; j++) matrix[j][0] = j

  for (let j = 1; j <= b.length; j++) {
    for (let i = 1; i <= a.length; i++) {
      const cost = a[i - 1] === b[j - 1] ? 0 : 1
      matrix[j][i] = Math.min(
        matrix[j][i - 1] + 1,
        matrix[j - 1][i] + 1,
        matrix[j - 1][i - 1] + cost
      )
    }
  }

  return matrix[b.length][a.length]
}

function getTokenClassName(token: SyntaxToken): string {
  const baseClasses = "inline"
  
  if (!token.valid) {
    return `${baseClasses} text-red-600 bg-red-100 dark:bg-red-900/20 underline decoration-wavy decoration-red-500`
  }

  switch (token.type) {
    case 'field':
      return `${baseClasses} text-blue-600 dark:text-blue-400 font-medium`
    case 'operator':
      return `${baseClasses} text-purple-600 dark:text-purple-400 font-medium`
    case 'value':
      return `${baseClasses} text-green-600 dark:text-green-400`
    case 'logic':
      return `${baseClasses} text-orange-600 dark:text-orange-400 font-bold`
    case 'modifier':
      return `${baseClasses} text-pink-600 dark:text-pink-400 font-medium`
    case 'parenthesis':
      return `${baseClasses} text-gray-600 dark:text-gray-400 font-bold`
    case 'whitespace':
      return baseClasses
    default:
      return baseClasses
  }
}

function SyntaxHelp() {
  return (
    <Popover>
      <PopoverTrigger asChild>
        <Button variant="ghost" size="sm" className="h-6 w-6 p-0">
          <HelpCircle className="h-3 w-3" />
        </Button>
      </PopoverTrigger>
      <PopoverContent className="w-96 popover-backdrop" side="top">
        <div className="space-y-4">
          <div>
            <h4 className="font-medium mb-2 flex items-center gap-2">
              <Code2 className="h-4 w-4" />
              Operators
            </h4>
            <div className="grid grid-cols-2 gap-1 text-xs">
              {KEYWORDS.operators.slice(0, 6).map((op, idx) => (
                <Badge key={`op-${op}-${idx}`} variant="outline" className="justify-start text-xs">
                  {op}
                </Badge>
              ))}
            </div>
          </div>
          
          <div>
            <h4 className="font-medium mb-2 flex items-center gap-2">
              <Zap className="h-4 w-4" />
              Logic & Modifiers
            </h4>
            <div className="flex flex-wrap gap-1 text-xs">
              {[...KEYWORDS.logic, ...KEYWORDS.modifiers].map((keyword, index) => (
                <Badge key={`${keyword}-${index}`} variant="outline" className="text-xs">
                  {keyword}
                </Badge>
              ))}
            </div>
          </div>

          <div>
            <h4 className="font-medium mb-2 flex items-center gap-2">
              <Lightbulb className="h-4 w-4" />
              Examples
            </h4>
            <div className="space-y-1 text-xs font-mono">
              {SYNTAX_EXAMPLES.slice(0, 3).map((example, idx) => (
                <div key={`help-example-${idx}-${example.slice(0, 10)}`} className="p-2 bg-muted rounded text-xs truncate">
                  {example}
                </div>
              ))}
            </div>
          </div>
        </div>
      </PopoverContent>
    </Popover>
  )
}

export function FilterSyntaxEditor({
  value,
  onChange,
  placeholder = "Enter filter expression...",
  disabled = false,
  className = ""
}: FilterSyntaxEditorProps) {
  const [validation, setValidation] = useState<ValidationResult | null>(null)
  const [isValidating, setIsValidating] = useState(false)
  const [availableFields, setAvailableFields] = useState<string[]>(COMMON_FIELDS)
  const [tokens, setTokens] = useState<SyntaxToken[]>([])
  const [showAutocomplete, setShowAutocomplete] = useState(false)
  const [autocompletePosition, setAutocompletePosition] = useState({ x: 0, y: 0 })
  const [autocompleteOptions, setAutocompleteOptions] = useState<string[]>([])
  const [currentWordStart, setCurrentWordStart] = useState(0)
  
  const editorRef = useRef<HTMLDivElement>(null)
  const validationTimeoutRef = useRef<NodeJS.Timeout | null>(null)

  // Load available fields on mount
  useEffect(() => {
    const loadFields = async () => {
      try {
        const fields = await apiClient.getFilterFields()
        setAvailableFields([...fields, ...COMMON_FIELDS])
      } catch (error) {
        console.error('Failed to load filter fields:', error)
      }
    }
    loadFields()
  }, [])

  // Tokenize and highlight syntax
  useEffect(() => {
    const newTokens = tokenizeExpression(value, availableFields)
    setTokens(newTokens)
  }, [value, availableFields])

  // Validate expression with debounce
  const validateExpression = useCallback(async (expression: string) => {
    if (!expression.trim()) {
      setValidation(null)
      return
    }

    setIsValidating(true)
    try {
      const result = await apiClient.validateFilter(expression)
      setValidation({ ...result, tokens })
    } catch (error) {
      setValidation({
        valid: false,
        error: error instanceof Error ? error.message : 'Validation failed',
        tokens
      })
    } finally {
      setIsValidating(false)
    }
  }, [tokens])

  useEffect(() => {
    if (validationTimeoutRef.current) {
      clearTimeout(validationTimeoutRef.current)
    }

    validationTimeoutRef.current = setTimeout(() => {
      validateExpression(value)
    }, 500)

    return () => {
      if (validationTimeoutRef.current) {
        clearTimeout(validationTimeoutRef.current)
      }
    }
  }, [value, validateExpression])

  const handleInput = (e: React.FormEvent<HTMLDivElement>) => {
    const newValue = e.currentTarget.textContent || ''
    onChange(newValue)
  }

  const handleKeyDown = (e: React.KeyboardEvent<HTMLDivElement>) => {
    if (e.key === 'Tab') {
      e.preventDefault()
      // Handle tab completion
    } else if (e.key === 'Enter' && e.ctrlKey) {
      e.preventDefault()
      // Trigger validation
      validateExpression(value)
    }
  }

  const insertAtCursor = (text: string) => {
    if (!editorRef.current) return
    
    const selection = window.getSelection()
    if (selection && selection.rangeCount > 0) {
      const range = selection.getRangeAt(0)
      range.deleteContents()
      range.insertNode(document.createTextNode(text))
      range.collapse(false)
      
      // Update the value
      const newValue = editorRef.current.textContent || ''
      onChange(newValue)
    }
  }

  const insertExample = (example: string) => {
    onChange(example)
    if (editorRef.current) {
      editorRef.current.focus()
    }
  }

  const renderHighlightedContent = () => {
    return tokens.map((token, index) => (
      <span
        key={`${token.start}-${token.end}-${String(token.value || '')}-${index}`}
        className={getTokenClassName(token)}
        title={!token.valid && token.suggestion ? `Did you mean: ${token.suggestion}?` : undefined}
      >
        {token.value}
      </span>
    ))
  }

  return (
    <TooltipProvider>
      <div className={`space-y-3 ${className}`}>
        {/* Header with validation status */}
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-2">
            <div className="flex items-center gap-2">
              <Code2 className="h-4 w-4" />
              <span className="text-sm font-medium">Filter Expression</span>
            </div>
            {validation && (
              <div className="flex items-center gap-1">
                {validation.valid ? (
                  <Tooltip>
                    <TooltipTrigger>
                      <CheckCircle className="h-4 w-4 text-green-600" />
                    </TooltipTrigger>
                    <TooltipContent>
                      <p>Valid expression</p>
                      {typeof validation.match_count === 'number' && (
                        <p>Matches {validation.match_count} record(s)</p>
                      )}
                    </TooltipContent>
                  </Tooltip>
                ) : (
                  <Tooltip>
                    <TooltipTrigger>
                      <XCircle className="h-4 w-4 text-red-600" />
                    </TooltipTrigger>
                    <TooltipContent>
                      <p>Invalid expression</p>
                      {validation.error && <p className="text-xs">{validation.error}</p>}
                    </TooltipContent>
                  </Tooltip>
                )}
                {typeof validation.match_count === 'number' && (
                  <Badge variant="outline" className="text-xs">
                    <Hash className="h-3 w-3 mr-1" />
                    {validation.match_count}
                  </Badge>
                )}
              </div>
            )}
            {isValidating && (
              <div className="h-4 w-4 border-2 border-blue-600 border-t-transparent rounded-full animate-spin" />
            )}
          </div>
          <SyntaxHelp />
        </div>

        {/* Main editor */}
        <Card>
          <CardContent className="p-4">
            <div className="relative">
              {/* Backdrop for syntax highlighting */}
              <div className="absolute inset-0 pointer-events-none font-mono text-sm leading-6 whitespace-pre-wrap break-words z-10 p-3 text-transparent">
                {renderHighlightedContent()}
              </div>
              
              {/* Editable content */}
              <div
                ref={editorRef}
                contentEditable={!disabled}
                onInput={handleInput}
                onKeyDown={handleKeyDown}
                className="relative z-20 min-h-[120px] w-full font-mono text-sm leading-6 whitespace-pre-wrap break-words p-3 bg-transparent outline-none resize-none caret-blue-600"
                style={{ color: 'transparent' }}
                suppressContentEditableWarning={true}
                data-placeholder={!value ? placeholder : undefined}
              />
              
              {/* Placeholder */}
              {!value && (
                <div className="absolute top-3 left-3 pointer-events-none text-muted-foreground font-mono text-sm">
                  {placeholder}
                </div>
              )}
            </div>
          </CardContent>
        </Card>

        {/* Validation error */}
        {validation && !validation.valid && validation.error && (
          <Alert variant="destructive">
            <XCircle className="h-4 w-4" />
            <AlertDescription>
              <strong>Syntax Error:</strong> {validation.error}
            </AlertDescription>
          </Alert>
        )}

        {/* Helper panels */}
        <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
          {/* Available Fields */}
          <Card>
            <CardHeader className="pb-2">
              <CardTitle className="text-sm flex items-center gap-2">
                <Type className="h-3 w-3" />
                Fields
              </CardTitle>
            </CardHeader>
            <CardContent className="pt-0">
              <div className="flex flex-wrap gap-1">
                {availableFields.slice(0, 6).map((field, idx) => (
                  <Tooltip key={`field-${field}-${idx}`}>
                    <TooltipTrigger asChild>
                      <Badge 
                        variant="outline" 
                        className="text-xs cursor-pointer hover:bg-accent"
                        onClick={() => insertAtCursor(field)}
                      >
                        {field}
                      </Badge>
                    </TooltipTrigger>
                    <TooltipContent>
                      <p>Click to insert: {field}</p>
                    </TooltipContent>
                  </Tooltip>
                ))}
                {availableFields.length > 6 && (
                  <Badge variant="outline" className="text-xs">
                    +{availableFields.length - 6}
                  </Badge>
                )}
              </div>
            </CardContent>
          </Card>

          {/* Quick Operators */}
          <Card>
            <CardHeader className="pb-2">
              <CardTitle className="text-sm flex items-center gap-2">
                <Settings className="h-3 w-3" />
                Operators
              </CardTitle>
            </CardHeader>
            <CardContent className="pt-0">
              <div className="flex flex-wrap gap-1">
                {['contains', 'equals', 'matches', 'AND', 'OR'].map((op, idx) => (
                  <Badge 
                    key={`quick-op-${op}-${idx}`}
                    variant="outline" 
                    className="text-xs cursor-pointer hover:bg-accent"
                    onClick={() => insertAtCursor(` ${op} `)}
                  >
                    {op}
                  </Badge>
                ))}
              </div>
            </CardContent>
          </Card>

          {/* Examples */}
          <Card>
            <CardHeader className="pb-2">
              <CardTitle className="text-sm flex items-center gap-2">
                <Lightbulb className="h-3 w-3" />
                Examples
              </CardTitle>
            </CardHeader>
            <CardContent className="pt-0">
              <div className="space-y-1">
                {SYNTAX_EXAMPLES.slice(0, 2).map((example, idx) => (
                  <Tooltip key={`panel-example-${idx}-${example.slice(0, 15)}`}>
                    <TooltipTrigger asChild>
                      <div 
                        className="text-xs font-mono p-1 bg-muted rounded cursor-pointer hover:bg-accent truncate"
                        onClick={() => insertExample(example)}
                      >
                        {example}
                      </div>
                    </TooltipTrigger>
                    <TooltipContent>
                      <p>Click to use this example</p>
                    </TooltipContent>
                  </Tooltip>
                ))}
              </div>
            </CardContent>
          </Card>
        </div>

        {/* Keyboard shortcuts */}
        <div className="text-xs text-muted-foreground">
          <kbd className="px-1.5 py-0.5 bg-muted rounded text-xs">Ctrl</kbd> + <kbd className="px-1.5 py-0.5 bg-muted rounded text-xs">Enter</kbd> to validate
        </div>
      </div>
    </TooltipProvider>
  )
}