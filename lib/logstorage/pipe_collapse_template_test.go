package logstorage

import "testing"

func TestParsePipeCollapseTemplateSuccess(t *testing.T) {
	f := func(pipeStr string) {
		t.Helper()
		expectParsePipeSuccess(t, pipeStr)
	}

	f(`collapse_template "/v<N>/orders/<ID>/courier"`)
	f(`collapse_template "api.example.com/v<N>/orders/<ID>/courier" at http.url`)
	f(`collapse_template "v<N>.service.<W>" split "." at host`)
}

func TestParsePipeCollapseTemplateFailure(t *testing.T) {
	f := func(pipeStr string) {
		t.Helper()
		expectParsePipeFailure(t, pipeStr)
	}

	f(`collapse_template`)
	f(`collapse_template /v<N>`)              // unquoted template
	f(`collapse_template "/v<X>/orders"`)     // unsupported placeholder
	f(`collapse_template "/v<N>/orders" foo`) // garbage tail
	f(`collapse_template "/v<N>" split ""`)   // empty separator
}

func TestPipeCollapseTemplate(t *testing.T) {
	f := func(pipeStr string, rows, rowsExpected [][]Field) {
		t.Helper()
		expectPipeResults(t, pipeStr, rows, rowsExpected)
	}

	f(`collapse_template "/v<N>/orders/<ID>/courier" at path`, [][]Field{
		{
			{"path", "/v2/orders/eW-CM198765432/courier"},
		},
		{
			{"path", "/v3/orders/abc123/courier"},
		},
		{
			{"path", "/other"},
		},
	}, [][]Field{
		{
			{"path", "/v<N>/orders/<ID>/courier"},
		},
		{
			{"path", "/v<N>/orders/<ID>/courier"},
		},
		{
			{"path", "/other"},
		},
	})

	// Non-slash separator (dot)
	f(`collapse_template "v<N>.service.<W>" split "." at host`, [][]Field{
		{
			{"host", "v12.service.api"},
		},
		{
			{"host", "v3.service.storage"},
		},
		{
			{"host", "foo.bar"},
		},
	}, [][]Field{
		{
			{"host", "v<N>.service.<W>"},
		},
		{
			{"host", "v<N>.service.<W>"},
		},
		{
			{"host", "foo.bar"},
		},
	})

	// Non-slash separator (colon) with prefix/suffix
	f(`collapse_template "<W>:v<N>:rev<ID>" split ":" at tag`, [][]Field{
		{
			{"tag", "svc:v12:revabc"},
		},
		{
			{"tag", "svc:v1:rev42"},
		},
		{
			{"tag", "svc:v1:other"},
		},
	}, [][]Field{
		{
			{"tag", "<W>:v<N>:rev<ID>"},
		},
		{
			{"tag", "<W>:v<N>:rev<ID>"},
		},
		{
			{"tag", "svc:v1:other"},
		},
	})

	// Unmatched should pass through unchanged
	f(`collapse_template "/v<N>/orders/<ID>" at path`, [][]Field{
		{
			{"path", "/foo/bar"},
		},
	}, [][]Field{
		{
			{"path", "/foo/bar"},
		},
	})
}

func TestPipeCollapseTemplateWithStatsAndSort(t *testing.T) {
	query := `collapse_template "api.example.com/v<N>/orders/<ID>/courier" at http.url | stats by (http.url) count() as hits | sort by (hits desc)`
	rows := [][]Field{
		{
			{"http.url", "api.example.com/"},
		},
		{
			{"http.url", "api.example.com/v2/orders/eW-CM198765432/courier"},
		},
		{
			{"http.url", "api.example.com/v3/orders/eW-CM999000111/courier"},
		},
	}
	expected := [][]Field{
		{
			{"http.url", "api.example.com/v<N>/orders/<ID>/courier"},
			{"hits", "2"},
		},
		{
			{"http.url", "api.example.com/"},
			{"hits", "1"},
		},
	}

	expectPipelineResults(t, query, rows, expected)
}
