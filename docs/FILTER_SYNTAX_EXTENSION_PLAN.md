# Filter Syntax Extension Plan

## Overview

This document outlines the plan to extend the elegant filter syntax system to support data mapping operations, providing a unified natural language interface for both filtering and data transformation.

## 🐛 Bug Fix Completed

**Issue**: The `starts_with` operator was being converted to `"starts with"` (with space) in the display function, causing inconsistency with the rest of the system that expects `"starts_with"` (with underscore).

**Solution**: Fixed `formatOperatorDisplay` function in `filters.js` to maintain underscore consistency:
- `starts_with: "starts_with"` (was: `"starts with"`)
- `ends_with: "ends_with"` (was: `"ends with"`)

## 📊 Current State Analysis

### Filter System (Strengths to Preserve)
- **Natural Language Syntax**: `channel_name contains "sport" AND group_title not contains "adult"`
- **Rich Operators**: `contains`, `equals`, `matches`, `starts_with`, `ends_with`
- **Modifiers**: `not`, `case_sensitive`
- **Logic**: `AND`, `OR` with proper precedence
- **Excellent UX**: Real-time validation, syntax highlighting, error reporting
- **Intuitive**: Readable by non-technical users

### Data Mapping System (Current Limitations)
- **Dropdown-based UI**: Separate controls for fields, operators, values
- **Disconnected Experience**: Different interface paradigm from filters
- **Limited Expressiveness**: Complex conditions require multiple rows
- **Maintenance Overhead**: Harder to version, share, and document rules

## 🎯 Goals

1. **Unified Experience**: Same syntax for both filtering and data mapping
2. **Enhanced Power**: Support both conditions and actions in one expression
3. **Backward Compatibility**: Existing filters continue to work unchanged
4. **Gradual Migration**: Smooth transition path for existing data mapping rules
5. **Improved Maintainability**: Text-based rules that are versionable and shareable

## 🚀 Implementation Phases

### Phase 1: Core Infrastructure ✅
- [x] Fix `starts_with` operator consistency bug
- [ ] Comprehensive testing of all filter operators
- [ ] Validation of existing filter syntax parsing
- [ ] Performance baseline establishment

### Phase 2: Syntax Design & Specification
- [ ] Define extended syntax grammar
- [ ] Design action syntax patterns
- [ ] Create comprehensive syntax documentation
- [ ] Validate syntax with complex use cases

### Phase 3: Parser Extension
- [ ] Extend lexer to handle action tokens
- [ ] Implement action parsing logic
- [ ] Add semantic validation for actions
- [ ] Create comprehensive test suite

### Phase 4: UI Components
- [ ] Enhanced text editor with syntax highlighting
- [ ] Real-time validation and error reporting
- [ ] Auto-completion for fields and values
- [ ] Preview functionality for rule effects

### Phase 5: Data Mapping Integration
- [ ] Implement rule execution engine
- [ ] Create migration utilities
- [ ] Add dual-mode UI (syntax/form)
- [ ] Comprehensive testing with real data

### Phase 6: Migration & Rollout
- [ ] Gradual rollout with feature flags
- [ ] User training and documentation
- [ ] Performance monitoring
- [ ] Legacy cleanup

## 📝 Extended Syntax Specification

### Current Filter Syntax (Preserved)
```
channel_name contains "sport" AND group_title not contains "adult"
channel_name case_sensitive equals "BBC One"
(channel_name matches "^HD" OR channel_name matches "4K$") AND language equals "en"
```

### Extended Syntax (New)

#### Basic Action Syntax
```
condition_expression SET field = "value"
```

#### Multiple Actions
```
condition_expression SET field1 = "value1", field2 = "value2"
```

#### Assignment Operators
- `=` : Set value (overwrites)
- `+=` : Append to existing value
- `?=` : Set only if field is empty
- `-=` : Remove from existing value

#### Complex Examples
```
// Simple default assignment
group_title equals "" SET group_title = "Uncategorized"

// Conditional categorization
channel_name contains "sport" SET group_title = "Sports", category = "entertainment"

// Multiple conditions with actions
(channel_name contains "sport" OR channel_name contains "football") AND language equals "en" 
SET group_title = "English Sports", logo_url = "sports-logo.png"

// Conditional appending
channel_name contains "HD" SET channel_name += " [HD Quality]"

// Logo assignment based on provider
tvg_id starts_with "bbc" SET logo_url = "https://logos.example.com/bbc.png"

// Complex transformations
channel_name matches "^([^|]+)\\s*\\|\\s*(.+)$" SET 
  channel_name = "$1", 
  group_title = "$2"
```

### Action Field Types

#### Stream Source Fields
- `channel_name` - Channel display name
- `group_title` - Channel group/category
- `tvg_id` - Electronic Program Guide ID
- `tvg_logo` - Logo URL
- `language` - Channel language
- `stream_url` - Stream URL (advanced)
- `category` - Custom category field

#### EPG Source Fields
- `program_title` - Program name
- `program_description` - Program description
- `category` - Program category
- `language` - Program language

### Special Functions (Future)
```
// String manipulation
channel_name contains "sport" SET group_title = upper("Sports")
channel_name matches "(.+)\\s+HD$" SET channel_name = trim("$1")

// Conditional logic
group_title equals "" SET group_title = if(language equals "en", "English", "International")

// External data lookup
tvg_id starts_with "bbc" SET logo_url = lookup_logo(tvg_id)
```

## 🔧 Technical Implementation Details

### Parser Architecture
```
Input: "channel_name contains 'sport' SET group_title = 'Sports'"
        ↓
Lexer: [IDENTIFIER, OPERATOR, STRING, KEYWORD, IDENTIFIER, EQUALS, STRING]
        ↓
Parser: ConditionExpression { field, operator, value, actions: [Action] }
        ↓
Validator: Semantic validation of fields, operators, values
        ↓
Executor: Apply transformations to data
```

### Data Structures
```rust
// Rust-like pseudocode for clarity
struct ExtendedRule {
    condition: ConditionExpression,
    actions: Vec<Action>,
}

struct Action {
    field: String,
    operator: ActionOperator, // =, +=, ?=, -=
    value: ActionValue,
}

enum ActionValue {
    Literal(String),
    Variable(String),
    Function(String, Vec<ActionValue>),
}
```

### Validation Layers
1. **Syntax Validation**: Grammar correctness
2. **Semantic Validation**: Field existence, operator compatibility
3. **Value Validation**: Type checking, format validation
4. **Logic Validation**: Circular references, impossible conditions

## 🎨 User Experience Design

### Syntax Editor Features
- **Live Syntax Highlighting**: Keywords, operators, strings color-coded
- **Real-time Validation**: Immediate feedback on syntax errors
- **Auto-completion**: Context-aware suggestions for fields and values
- **Error Tooltips**: Detailed explanations of validation errors
- **Bracket Matching**: Visual matching of parentheses and quotes
- **Undo/Redo**: Full edit history support

### Preview Functionality
- **Rule Preview**: Show what the rule will do in plain English
- **Data Preview**: Show before/after data samples
- **Test Mode**: Run rules against sample data
- **Impact Analysis**: Show how many items would be affected

### Migration Tools
- **Import/Export**: Convert between syntax and form-based rules
- **Bulk Conversion**: Migrate existing rules automatically
- **Validation Report**: Identify potential issues before migration
- **Rollback Support**: Easily revert problematic changes

## 📊 Performance Considerations

### Optimization Strategies
- **Compiled Rules**: Pre-compile syntax to optimized execution trees
- **Caching**: Cache compiled rules and intermediate results
- **Batch Processing**: Process multiple items efficiently
- **Lazy Evaluation**: Only evaluate necessary conditions

### Scalability Targets
- **Rule Complexity**: Support 100+ condition/action combinations
- **Processing Speed**: Handle 10,000+ items per second
- **Memory Usage**: Minimize memory footprint for large datasets
- **Rule Count**: Support 1,000+ active rules per source

## 🧪 Testing Strategy

### Unit Testing
- **Parser Tests**: Comprehensive syntax parsing validation
- **Validator Tests**: All validation rules and edge cases
- **Executor Tests**: Action application correctness
- **Performance Tests**: Benchmarking and regression testing

### Integration Testing
- **End-to-End**: Full workflow from UI to data transformation
- **Migration Testing**: Conversion accuracy and completeness
- **Backward Compatibility**: Existing filters continue working
- **Error Handling**: Graceful degradation and recovery

### User Acceptance Testing
- **Usability Testing**: Interface effectiveness and intuitiveness
- **Performance Testing**: Real-world usage scenarios
- **Migration Testing**: Smooth transition for existing users
- **Documentation Testing**: Clarity and completeness of guides

## 📅 Timeline & Milestones

### Phase 1: Foundation (Week 1-2)
- Complete filter system validation
- Performance baseline establishment
- Test suite expansion

### Phase 2: Design (Week 3-4)
- Finalize syntax specification
- Create detailed technical design
- Prototype parser extensions

### Phase 3: Core Implementation (Week 5-8)
- Implement extended parser
- Create action execution engine
- Build validation system

### Phase 4: UI Development (Week 9-12)
- Enhanced syntax editor
- Preview and testing tools
- Migration utilities

### Phase 5: Integration (Week 13-16)
- Data mapping system integration
- Comprehensive testing
- Performance optimization

### Phase 6: Rollout (Week 17-20)
- Feature flag implementation
- User documentation
- Gradual migration support

## 🔒 Risk Mitigation

### Technical Risks
- **Parser Complexity**: Incremental development with comprehensive testing
- **Performance Impact**: Early performance testing and optimization
- **Backward Compatibility**: Extensive regression testing

### User Experience Risks
- **Learning Curve**: Comprehensive documentation and examples
- **Migration Complexity**: Automated tools and gradual transition
- **Feature Discoverability**: Intuitive UI design and guided workflows

### Operational Risks
- **Data Corruption**: Extensive validation and rollback capabilities
- **System Instability**: Feature flags and gradual rollout
- **User Resistance**: Clear benefits demonstration and training

## 📈 Success Metrics

### Quantitative Metrics
- **Adoption Rate**: % of users using new syntax vs. old forms
- **Performance**: Rule execution time and system responsiveness
- **Error Rate**: Syntax errors and validation failures
- **Migration Success**: % of rules successfully migrated

### Qualitative Metrics
- **User Satisfaction**: Feedback on ease of use and power
- **Support Tickets**: Reduction in configuration-related issues
- **Documentation Quality**: User feedback on clarity and completeness
- **Developer Experience**: Ease of extending and maintaining system

## 🎯 Future Enhancements

### Advanced Features
- **Visual Rule Builder**: Drag-and-drop interface for complex rules
- **Rule Templates**: Pre-built patterns for common use cases
- **Conditional Actions**: If-then-else logic within actions
- **External Integrations**: API calls and webhook actions

### Ecosystem Integration
- **Version Control**: Git-like versioning for rule sets
- **Collaboration**: Multi-user editing and review workflows
- **Import/Export**: Integration with external rule management tools
- **Analytics**: Usage patterns and effectiveness metrics

## 📚 Resources & References

### Documentation
- [Current Filter System Documentation](docs/FILTER_PREVIEW_IMPLEMENTATION.md)
- [Data Mapping Architecture](DATA_MAPPING_ARCHITECTURE_REFACTOR.md)
- [API Reference](API_REFACTORING_PLAN.md)

### Related Projects
- Similar syntax systems in other projects
- Parser generator tools and libraries
- UI component libraries for syntax highlighting

### Learning Resources
- Domain-specific language design patterns
- Parser implementation best practices
- User experience design for technical interfaces

---

*This document will be updated as the implementation progresses. For questions or suggestions, please refer to the project team.*