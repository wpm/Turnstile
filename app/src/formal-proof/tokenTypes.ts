/**
 * Maps an LSP semantic token type string to the CSS class used for
 * syntax highlighting in the editor. Returns null for unknown types.
 */
export function tokenTypeToCssClass(tokenType: string): string | null {
  switch (tokenType) {
    case 'keyword':
    case 'modifier':
      return 'cm-lean-keyword'
    case 'type':
    case 'class':
    case 'struct':
    case 'enum':
    case 'interface':
    case 'typeParameter':
      return 'cm-lean-type'
    case 'function':
    case 'method':
      return 'cm-lean-function'
    case 'variable':
    case 'parameter':
    case 'property':
      return 'cm-lean-variable'
    case 'namespace':
      return 'cm-lean-namespace'
    case 'enumMember':
      return 'cm-lean-enum-member'
    case 'macro':
      return 'cm-lean-macro'
    case 'comment':
      return 'cm-lean-comment'
    case 'string':
      return 'cm-lean-string'
    case 'number':
      return 'cm-lean-number'
    case 'operator':
      return 'cm-lean-operator'
    case 'decorator':
      return 'cm-lean-decorator'
    default:
      return null
  }
}
