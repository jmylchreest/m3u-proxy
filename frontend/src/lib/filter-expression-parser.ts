// Utility functions to convert between filter expressions and condition trees

export interface ConditionTreeNode {
  type: 'condition' | 'group'
  field?: string
  operator?: string
  value?: string
  case_sensitive?: boolean
  negate?: boolean
  children?: ConditionTreeNode[]
}

export interface ConditionTree {
  root: ConditionTreeNode
}

// Convert expression tree (from API) to readable text
export function expressionTreeToText(tree: any): string {
  if (!tree) return ''
  
  if (tree.type === 'condition') {
    const field = tree.field || ''
    const operator = formatOperatorForText(tree.operator || '')
    const value = tree.value || ''
    const caseSensitive = tree.case_sensitive ? ' case_sensitive' : ''
    const negate = tree.negate ? 'not ' : ''
    
    return `${negate}${caseSensitive} ${field} ${operator} "${value}"`.trim()
  }
  
  if (tree.type === 'group' && tree.children) {
    const operator = (tree.operator || 'OR').toUpperCase()
    const childExpressions = tree.children.map((child: any) => expressionTreeToText(child))
    
    if (childExpressions.length === 1) {
      return childExpressions[0]
    }
    
    const joined = childExpressions.join(` ${operator} `)
    
    // Add parentheses if this is a nested group
    return childExpressions.length > 1 ? `(${joined})` : joined
  }
  
  return ''
}

function formatOperatorForText(operator: string): string {
  switch (operator?.toLowerCase()) {
    case 'contains': return 'contains'
    case 'equals': return 'equals'
    case 'matches': return 'matches'
    case 'startswith': return 'starts_with'
    case 'endswith': return 'ends_with'
    case 'notcontains': return 'not_contains'
    case 'notequals': return 'not_equals'
    case 'notmatches': return 'not_matches'
    default: return operator?.toLowerCase() || ''
  }
}

// Simple parser to convert text to condition tree (basic implementation)
export function textToConditionTree(text: string): string {
  // For now, return the text as-is since the API might handle parsing
  // This is a placeholder for more sophisticated parsing
  
  if (!text.trim()) {
    return JSON.stringify({
      root: {
        type: 'condition',
        field: '',
        operator: '',
        value: ''
      }
    })
  }
  
  // Try to parse simple expressions
  const simpleConditionMatch = text.match(/^(\w+)\s+(contains|equals|matches|starts_with|ends_with|not_contains|not_equals|not_matches)\s+"([^"]*)"$/i)
  
  if (simpleConditionMatch) {
    const [, field, operator, value] = simpleConditionMatch
    return JSON.stringify({
      root: {
        type: 'condition',
        field: field.trim(),
        operator: operator.toLowerCase().replace('_', ''),
        value: value
      }
    })
  }
  
  // For complex expressions, return a placeholder structure
  // In a real implementation, you'd want a proper parser here
  return JSON.stringify({
    root: {
      type: 'condition',
      field: 'channel_name',
      operator: 'contains',
      value: text
    }
  })
}

// Extract a human-readable summary from condition tree
export function getConditionTreeSummary(conditionTree: string): string {
  try {
    const parsed = JSON.parse(conditionTree)
    if (parsed.root) {
      return getNodeSummary(parsed.root)
    }
    return 'Invalid condition tree'
  } catch (error) {
    return 'Parse error'
  }
}

function getNodeSummary(node: any): string {
  if (node.type === 'condition') {
    const field = node.field || 'field'
    const operator = node.operator || 'operator'
    const value = node.value || 'value'
    return `${field} ${operator} "${value}"`
  }
  
  if (node.type === 'group' && node.children) {
    const operator = (node.operator || 'OR').toUpperCase()
    const summaries = node.children.map(getNodeSummary)
    
    if (summaries.length === 1) return summaries[0]
    if (summaries.length <= 2) return summaries.join(` ${operator} `)
    
    return `${summaries.length} conditions with ${operator}`
  }
  
  return 'Unknown'
}