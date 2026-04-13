import { describe, it, expect } from 'vitest'
import { renderContent } from './renderContent'

describe('renderContent', () => {
  // ── Markdown basics ──────────────────────────────────────────────────

  describe('markdown basics', () => {
    it('renders headings', () => {
      expect(renderContent('# Title')).toContain('<h1')
      expect(renderContent('## Subtitle')).toContain('<h2')
      expect(renderContent('### H3')).toContain('<h3')
    })

    it('renders bold text', () => {
      const html = renderContent('**bold**')
      expect(html).toContain('<strong>')
      expect(html).toContain('bold')
    })

    it('renders italic text', () => {
      const html = renderContent('*italic*')
      expect(html).toContain('<em>')
      expect(html).toContain('italic')
    })

    it('renders unordered lists', () => {
      const html = renderContent('- one\n- two')
      expect(html).toContain('<ul>')
      expect(html).toContain('<li>')
    })

    it('renders ordered lists', () => {
      const html = renderContent('1. first\n2. second')
      expect(html).toContain('<ol>')
      expect(html).toContain('<li>')
    })

    it('renders links', () => {
      const html = renderContent('[click](https://example.com)')
      expect(html).toContain('<a ')
      expect(html).toContain('href="https://example.com"')
      expect(html).toContain('click')
    })

    it('renders blockquotes', () => {
      const html = renderContent('> quoted text')
      expect(html).toContain('<blockquote>')
    })

    it('renders single newlines as <br> (breaks mode)', () => {
      const html = renderContent('line one\nline two')
      expect(html).toContain('<br')
    })

    it('renders plain text in a paragraph', () => {
      const html = renderContent('hello world')
      expect(html).toContain('<p>')
      expect(html).toContain('hello world')
    })
  })

  // ── Fenced code blocks ──────────────────────────────────────────────

  describe('fenced code blocks', () => {
    it('highlights lean code with cm-lean-* classes', () => {
      const html = renderContent('```lean\ndef x := 1\n```')
      expect(html).toContain('<pre>')
      expect(html).toContain('chat-lean-code')
      expect(html).toContain('cm-lean-keyword')
    })

    it('defaults to lean highlighting when no language specified', () => {
      const html = renderContent('```\ndef x := 1\n```')
      expect(html).toContain('cm-lean-keyword')
    })

    it('does not apply lean highlighting for other languages', () => {
      const html = renderContent('```python\ndef foo():\n  pass\n```')
      expect(html).toContain('<pre>')
      expect(html).toContain('<code>')
      // Should NOT have cm-lean-keyword (python's `def` shouldn't get lean highlighting)
      expect(html).not.toContain('cm-lean-keyword')
    })
  })

  // ── Inline code ─────────────────────────────────────────────────────

  describe('inline code', () => {
    it('highlights lean keywords in inline code', () => {
      const html = renderContent('use `theorem` here')
      expect(html).toContain('<code')
      expect(html).toContain('chat-lean-code')
      expect(html).toContain('cm-lean-keyword')
    })
  })

  // ── Math preservation ───────────────────────────────────────────────

  describe('math', () => {
    it('renders inline math via KaTeX', () => {
      const html = renderContent('proof: $x + 1$')
      expect(html).toContain('katex')
    })

    it('renders display math via KaTeX', () => {
      const html = renderContent('$$\\frac{a}{b}$$')
      expect(html).toContain('katex')
      expect(html).toContain('katex-display')
    })

    it('does not process $ inside fenced code blocks as math', () => {
      const html = renderContent('```lean\ndef cost := $100\n```')
      // Should NOT contain katex rendering — the $ is inside a code fence
      expect(html).not.toContain('katex')
    })

    it('handles mixed markdown, math, and code', () => {
      const input = '**Theorem**: $a = b$\n\n```lean\ntheorem t : True := trivial\n```'
      const html = renderContent(input)
      expect(html).toContain('<strong>')
      expect(html).toContain('katex')
      expect(html).toContain('cm-lean-keyword')
    })
  })

  // ── XSS safety ──────────────────────────────────────────────────────

  describe('XSS safety', () => {
    it('escapes script tags in text', () => {
      const html = renderContent('<script>alert("xss")</script>')
      expect(html).not.toContain('<script>')
    })

    it('escapes HTML in code blocks', () => {
      const html = renderContent('```\n<img onerror=alert(1)>\n```')
      expect(html).not.toContain('<img')
    })
  })

  // ── Edge cases ──────────────────────────────────────────────────────

  describe('edge cases', () => {
    it('returns empty string for empty input', () => {
      expect(renderContent('')).toBe('')
    })

    it('handles inline code containing dollar signs without math', () => {
      const html = renderContent('`$x$`')
      // The $ should be treated as code, not math
      expect(html).toContain('chat-lean-code')
      expect(html).not.toContain('katex')
    })
  })
})
