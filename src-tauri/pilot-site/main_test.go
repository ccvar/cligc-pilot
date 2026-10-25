package main

import (
	"database/sql"
	"encoding/base64"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"net/url"
	"path/filepath"
	"strings"
	"testing"
)

func testApp(t *testing.T, cfg config) (http.Handler, *sql.DB) {
	t.Helper()
	db, err := openDB(filepath.Join(t.TempDir(), "site.db"))
	if err != nil {
		t.Fatal(err)
	}
	if cfg.ipSalt == "" {
		cfg.ipSalt = "x"
	}
	return newApp(db, cfg), db
}

func post(h http.Handler, path, ctype, body string, hdr map[string]string) *httptest.ResponseRecorder {
	r := httptest.NewRequest("POST", "http://example.com"+path, strings.NewReader(body))
	r.Host = "example.com"
	r.Header.Set("content-type", ctype)
	r.Header.Set("x-forwarded-for", "1.2.3.4")
	r.Header.Set("origin", "https://example.com")
	for k, v := range hdr {
		r.Header.Set(k, v)
	}
	w := httptest.NewRecorder()
	h.ServeHTTP(w, r)
	return w
}

func get(h http.Handler, path string) *httptest.ResponseRecorder {
	r := httptest.NewRequest("GET", "http://example.com"+path, nil)
	w := httptest.NewRecorder()
	h.ServeHTTP(w, r)
	return w
}

func body(w *httptest.ResponseRecorder) map[string]any {
	var m map[string]any
	_ = json.Unmarshal(w.Body.Bytes(), &m)
	return m
}

func TestForm(t *testing.T) {
	h, db := testApp(t, config{})
	const js = "application/json"
	w := post(h, "/api/form", js, `{"name":"张三","email":"a@b.c","message":"你好","_form":"contact","_page":"/contact/"}`, nil)
	if w.Code != 200 || body(w)["ok"] != true {
		t.Fatalf("json submit: %d %s", w.Code, w.Body)
	}
	var form, page, fields, ip string
	_ = db.QueryRow("SELECT form, page, fields, ip FROM submissions").Scan(&form, &page, &fields, &ip)
	if form != "contact" || page != "/contact/" || !strings.Contains(fields, "张三") || len(ip) != 24 || strings.Contains(ip, "1.2.3.4") {
		t.Fatalf("stored: %s %s %s %s", form, page, fields, ip)
	}
	// 普通表单 → 303 跳回来源页
	w = post(h, "/api/form", "application/x-www-form-urlencoded", url.Values{"email": {"x@y.z"}, "_form": {"subscribe"}}.Encode(), map[string]string{"accept": "text/html", "referer": "https://example.com/news/"})
	if w.Code != 303 || w.Header().Get("location") != "https://example.com/news/?sent=1" {
		t.Fatalf("form post: %d %s", w.Code, w.Header().Get("location"))
	}
	// 隐藏字段 → 假装成功不入库
	post(h, "/api/form", js, `{"email":"bot","_gotcha":"x"}`, nil)
	var n int
	_ = db.QueryRow("SELECT COUNT(*) FROM submissions").Scan(&n)
	if n != 2 {
		t.Fatalf("honeypot stored: %d", n)
	}
	// 跨站
	if w := post(h, "/api/form", js, `{"a":"1"}`, map[string]string{"origin": "https://evil.com"}); w.Code != 403 {
		t.Fatalf("cross origin: %d", w.Code)
	}
	// 限频
	for i := 0; i < 3; i++ {
		post(h, "/api/form", js, `{"m":"x"}`, nil)
	}
	if w := post(h, "/api/form", js, `{"m":"y"}`, nil); w.Code != 429 {
		t.Fatalf("rate limit: %d", w.Code)
	}
	// 空表单
	if w := post(h, "/api/form", js, `{"_form":"x"}`, map[string]string{"x-forwarded-for": "9.9.9.9"}); w.Code != 400 || body(w)["error"] != "empty" {
		t.Fatalf("empty: %d %s", w.Code, w.Body)
	}
}

func TestTurnstile(t *testing.T) {
	ok := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		_ = r.ParseForm()
		_, _ = w.Write([]byte(`{"success":` + map[bool]string{true: "true", false: "false"}[r.Form.Get("response") == "good"] + `}`))
	}))
	defer ok.Close()
	h, _ := testApp(t, config{turnstileSecret: "s", verifyURL: ok.URL})
	if w := post(h, "/api/form", "application/json", `{"m":"z"}`, nil); w.Code != 400 || body(w)["error"] != "captcha" {
		t.Fatalf("no token: %d %s", w.Code, w.Body)
	}
	if w := post(h, "/api/form", "application/json", `{"m":"z","cf-turnstile-response":"good"}`, nil); w.Code != 200 {
		t.Fatalf("good token: %d %s", w.Code, w.Body)
	}
}

func TestComments(t *testing.T) {
	h, db := testApp(t, config{})
	w := post(h, "/api/comments", "application/json", `{"path":"/a/","name":"李四","content":"写得好","url":"javascript:alert(1)","email":"l@x.y"}`, nil)
	if b := body(w); b["ok"] != true || b["pending"] != true {
		t.Fatalf("post: %s", w.Body)
	}
	if list := body(get(h, "/api/comments?path=/a/"))["comments"].([]any); len(list) != 0 {
		t.Fatalf("pending visible: %v", list)
	}
	var u string
	_ = db.QueryRow("SELECT url FROM comments").Scan(&u)
	if u != "" {
		t.Fatalf("js url kept: %q", u)
	}
	_, _ = db.Exec("UPDATE comments SET status = 'approved'")
	list := body(get(h, "/api/comments?path=/a/"))["comments"].([]any)
	c := list[0].(map[string]any)
	if len(list) != 1 || c["name"] != "李四" || c["email"] != nil || c["ip"] != nil {
		t.Fatalf("approved list: %v", list)
	}
	if w := post(h, "/api/comments", "application/json", `{"path":"a","name":"x","content":"yy"}`, nil); w.Code != 400 {
		t.Fatalf("bad path: %d", w.Code)
	}
	h2, _ := testApp(t, config{autoApprove: true})
	if b := body(post(h2, "/api/comments", "application/json", `{"path":"/a/","name":"x","content":"auto"}`, nil)); b["pending"] != false {
		t.Fatalf("auto approve: %v", b)
	}
}

func TestHealthAndAdmin(t *testing.T) {
	h, db := testApp(t, config{})
	if w := get(h, "/api/health"); w.Code != 200 || body(w)["db"] != true {
		t.Fatalf("health: %d %s", w.Code, w.Body)
	}
	post(h, "/api/form", "application/json", `{"message":"hi"}`, nil)
	q := func(s string) string { return base64.StdEncoding.EncodeToString([]byte(s)) }
	out, err := adminQuery(db, q(`{"sql":"SELECT id, form, fields, read FROM submissions","params":[]}`))
	if err != nil || !strings.Contains(string(out), `"form":"contact"`) || !strings.Contains(string(out), `"read":0`) {
		t.Fatalf("admin select: %s %v", out, err)
	}
	if out, err := adminQuery(db, q(`{"sql":"UPDATE submissions SET read = 1 WHERE id IN (?)","params":[1]}`)); err != nil || string(out) != "[]" {
		t.Fatalf("admin update: %s %v", out, err)
	}
	var r int
	_ = db.QueryRow("SELECT read FROM submissions WHERE id = 1").Scan(&r)
	if r != 1 {
		t.Fatal("update not applied")
	}
}
