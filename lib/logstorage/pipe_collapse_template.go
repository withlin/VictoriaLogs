package logstorage

import (
	"fmt"
	"strconv"
	"strings"

	"github.com/VictoriaMetrics/VictoriaLogs/lib/prefixfilter"
	"github.com/VictoriaMetrics/VictoriaMetrics/lib/bytesutil"
)

// pipeCollapseTemplate processes '| collapse_template ...' pipe.
//
// It normalizes string fields using simple placeholders without regex.
// Supported placeholders (can have prefix/suffix in a segment):
// - <N>  : digits
// - <ID> : any non-empty segment without the separator
// - <W>  : word segment (letters, digits, underscore)
type pipeCollapseTemplate struct {
	field    string
	template *valueTemplate
	iff      *ifFilter
}

func (pct *pipeCollapseTemplate) String() string {
	s := "collapse_template " + quoteTokenIfNeeded(pct.template.raw)
	if pct.iff != nil {
		s += " " + pct.iff.String()
	}
	if pct.template.sep != "/" {
		s += " split " + strconv.Quote(pct.template.sep)
	}
	if pct.field != "_msg" {
		s += " at " + quoteTokenIfNeeded(pct.field)
	}
	return s
}

func (pct *pipeCollapseTemplate) splitToRemoteAndLocal(_ int64) (pipe, []pipe) {
	return pct, nil
}

func (pct *pipeCollapseTemplate) canLiveTail() bool {
	return true
}

func (pct *pipeCollapseTemplate) canReturnLastNResults() bool {
	return true
}

func (pct *pipeCollapseTemplate) updateNeededFields(pf *prefixfilter.Filter) {
	updateNeededFieldsForUpdatePipe(pf, pct.field, pct.iff)
}

func (pct *pipeCollapseTemplate) hasFilterInWithQuery() bool {
	return pct.iff.hasFilterInWithQuery()
}

func (pct *pipeCollapseTemplate) visitSubqueries(visitFunc func(q *Query)) {
	pct.iff.visitSubqueries(visitFunc)
}

func (pct *pipeCollapseTemplate) initFilterInValues(cache *inValuesCache, getFieldValuesFunc getFieldValuesFunc, keepSubquery bool) (pipe, error) {
	iffNew, err := pct.iff.initFilterInValues(cache, getFieldValuesFunc, keepSubquery)
	if err != nil {
		return nil, err
	}
	pctNew := *pct
	pctNew.iff = iffNew
	return &pctNew, nil
}

func (pct *pipeCollapseTemplate) newPipeProcessor(_ int, _ <-chan struct{}, _ func(), ppNext pipeProcessor) pipeProcessor {
	updateFunc := func(a *arena, v string) string {
		if normalized, ok := pct.template.collapse(v); ok {
			bLen := len(a.b)
			a.b = append(a.b, normalized...)
			return bytesutil.ToUnsafeString(a.b[bLen:])
		}
		return v
	}
	return newPipeUpdateProcessor(updateFunc, ppNext, pct.field, pct.iff)
}

func parsePipeCollapseTemplate(lex *lexer) (pipe, error) {
	if !lex.isKeyword("collapse_template") {
		return nil, fmt.Errorf("unexpected token: %q; want %q", lex.token, "collapse_template")
	}
	lex.nextToken()

	// read template
	tmplStr, err := lex.nextCompoundToken()
	if err != nil {
		return nil, fmt.Errorf("cannot parse template for collapse_template: %w", err)
	}

	sep := "/"
	if lex.isKeyword("split") {
		lex.nextToken()
		sep, err = lex.nextCompoundToken()
		if err != nil {
			return nil, fmt.Errorf("cannot parse split separator for collapse_template: %w", err)
		}
		if sep == "" {
			return nil, fmt.Errorf("split separator cannot be empty")
		}
	}

	tmpl, err := parseValueTemplate(tmplStr, sep)
	if err != nil {
		return nil, fmt.Errorf("invalid collapse_template template %q: %w", tmplStr, err)
	}

	// optional if (...)
	var iff *ifFilter
	if lex.isKeyword("if") {
		f, err := parseIfFilter(lex)
		if err != nil {
			return nil, err
		}
		iff = f
	}

	field := "_msg"
	if lex.isKeyword("at") {
		lex.nextToken()
		f, err := parseFieldName(lex)
		if err != nil {
			return nil, fmt.Errorf("cannot parse 'at' field after collapse_template: %w", err)
		}
		field = f
	}

	pct := &pipeCollapseTemplate{
		field:    field,
		template: tmpl,
		iff:      iff,
	}
	return pct, nil
}

type templateSegmentKind int

const (
	templSegLiteral templateSegmentKind = iota
	templSegN
	templSegID
	templSegW
)

type templateSegment struct {
	kind    templateSegmentKind
	literal string
	prefix  string
	suffix  string
}

type valueTemplate struct {
	segments   []templateSegment
	raw        string
	normalized string
	sep        string
}

func parseValueTemplate(s, sep string) (*valueTemplate, error) {
	parts := strings.Split(s, sep)
	segments := make([]templateSegment, len(parts))
	normParts := make([]string, len(parts))
	for i, p := range parts {
		seg, norm, err := parseTemplateSegment(p)
		if err != nil {
			return nil, err
		}
		segments[i] = seg
		normParts[i] = norm
	}

	return &valueTemplate{
		segments:   segments,
		raw:        s,
		normalized: strings.Join(normParts, sep),
		sep:        sep,
	}, nil
}

func (vt *valueTemplate) collapse(s string) (string, bool) {
	parts := strings.Split(s, vt.sep)
	if len(parts) != len(vt.segments) {
		return s, false
	}
	for i, seg := range vt.segments {
		val := parts[i]
		switch seg.kind {
		case templSegLiteral:
			if val != seg.literal {
				return s, false
			}
		case templSegN, templSegID, templSegW:
			if !matchesTemplatePlaceholder(seg, val) {
				return s, false
			}
		default:
			return s, false
		}
	}
	return vt.normalized, true
}

func parseTemplateSegment(s string) (templateSegment, string, error) {
	if s == "" {
		return templateSegment{kind: templSegLiteral, literal: s}, s, nil
	}

	placeholderDefs := []struct {
		token string
		kind  templateSegmentKind
	}{
		{"<N>", templSegN},
		{"<ID>", templSegID},
		{"<W>", templSegW},
	}

	for _, def := range placeholderDefs {
		if strings.Contains(s, def.token) {
			if strings.Count(s, "<") > 1 || strings.Count(s, ">") > 1 {
				return templateSegment{}, "", fmt.Errorf("unexpected placeholder %q; supported: <N>, <ID>, <W>", s)
			}
			idx := strings.Index(s, def.token)
			prefix := s[:idx]
			suffix := s[idx+len(def.token):]
			return templateSegment{kind: def.kind, prefix: prefix, suffix: suffix}, prefix + def.token + suffix, nil
		}
	}

	if strings.Contains(s, "<") || strings.Contains(s, ">") {
		return templateSegment{}, "", fmt.Errorf("unexpected placeholder %q; supported: <N>, <ID>, <W>", s)
	}

	return templateSegment{kind: templSegLiteral, literal: s}, s, nil
}

func matchesTemplatePlaceholder(seg templateSegment, value string) bool {
	if len(value) < len(seg.prefix)+len(seg.suffix) {
		return false
	}
	if seg.prefix != "" && !strings.HasPrefix(value, seg.prefix) {
		return false
	}
	if seg.suffix != "" && !strings.HasSuffix(value, seg.suffix) {
		return false
	}

	start := len(seg.prefix)
	end := len(value) - len(seg.suffix)
	if start > end {
		return false
	}
	inner := value[start:end]
	if inner == "" {
		return false
	}

	switch seg.kind {
	case templSegN:
		return isDigitsTemplate(inner)
	case templSegID:
		return true
	case templSegW:
		return isWordSegmentTemplate(inner)
	default:
		return false
	}
}

func isDigitsTemplate(s string) bool {
	for i := 0; i < len(s); i++ {
		if s[i] < '0' || s[i] > '9' {
			return false
		}
	}
	return true
}

func isWordSegmentTemplate(s string) bool {
	for i := 0; i < len(s); i++ {
		if !isTokenChar(s[i]) {
			return false
		}
	}
	return true
}
