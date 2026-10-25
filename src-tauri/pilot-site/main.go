// pilot-site：Cligc Pilot 部署在自己服务器上的站点小服务（Caddy 把 /api/* 转过来）。
//
// 接口和 Cloudflare 上的 Pages Functions（src-tauri/src/builtin/functions）完全一致，前端不用区分部署在哪：
//
//	POST /api/form       收表单（_form 表单名、_page 来源页、_gotcha 防机器人隐藏字段；普通表单提交跳回原页带 ?sent=1）
//	GET  /api/comments   ?path= 这一页审核通过的评论
//	POST /api/comments   发评论（默认待审核，COMMENTS_AUTO_APPROVE=1 直接通过）
//	GET  /api/health     服务和数据库是否正常
//
// 数据存 SQLite（纯 Go 驱动，单个可执行文件，服务器上不用装别的）。访客 IP 只存哈希。
//
//	pilot-site serve --port 18123 --db /var/www/pilot/<站点>/data/site.db
//	pilot-site admin --db <路径> <base64(JSON {sql, params})>   Pilot 通过 SSH 读写收件箱，打印 JSON
package main

import (
	"crypto/sha256"
	"database/sql"
	_ "embed"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"log"
	"net"
	"net/http"
	"net/url"
	"os"
	"regexp"
	"strconv"
	"strings"
	"time"

	_ "modernc.org/sqlite"
)

//go:embed schema.sql
var schema string

func main() {
	if len(os.Args) < 2 {
		fmt.Fprintln(os.Stderr, "用法: pilot-site serve --port N --db PATH | pilot-site admin --db PATH <base64>")
		os.Exit(2)
	}
	switch os.Args[1] {
	case "serve":
		fs := flag.NewFlagSet("serve", flag.ExitOnError)
		port := fs.Int("port", 18080, "监听端口（只监听 127.0.0.1）")
		dbPath := fs.String("db", "site.db", "SQLite 文件")
		_ = fs.Parse(os.Args[2:])
		db, err := openDB(*dbPath)
		if err != nil {
			log.Fatal(err)
		}
		srv := &http.Server{Addr: fmt.Sprintf("127.0.0.1:%d", *port), Handler: newApp(db, envConfig()), ReadTimeout: 15 * time.Second, WriteTimeout: 15 * time.Second}
		log.Printf("pilot-site on %s", srv.Addr)
		log.Fatal(srv.ListenAndServe())
	case "admin":
		fs := flag.NewFlagSet("admin", flag.ExitOnError)
		dbPath := fs.String("db", "site.db", "SQLite 文件")
		_ = fs.Parse(os.Args[2:])
		if fs.NArg() < 1 {
			log.Fatal("缺少查询参数")
		}
		db, err := openDB(*dbPath)
		if err != nil {
			log.Fatal(err)
		}
		out, err := adminQuery(db, fs.Arg(0))
		if err != nil {
			log.Fatal(err)
		}
		os.Stdout.Write(out)
	case "version":
		fmt.Println("pilot-site 1")
	default:
		fmt.Fprintln(os.Stderr, "未知命令", os.Args[1])
		os.Exit(2)
	}
}

func openDB(path string) (*sql.DB, error) {
	db, err := sql.Open("sqlite", path+"?_pragma=busy_timeout(5000)&_pragma=journal_mode(WAL)")
	if err != nil {
		return nil, err
	}
	db.SetMaxOpenConns(1)
	if _, err := db.Exec(schema); err != nil {
		return nil, fmt.Errorf("建表失败: %w", err)
	}
	return db, nil
}

// adminQuery：执行 Pilot 发来的一条 SQL（参数化），SELECT 返回结果行，其它返回 []。
func adminQuery(db *sql.DB, b64 string) ([]byte, error) {
	raw, err := base64.StdEncoding.DecodeString(b64)
	if err != nil {
		return nil, err
	}
	var q struct {
		SQL    string `json:"sql"`
		Params []any  `json:"params"`
	}
	if err := json.Unmarshal(raw, &q); err != nil {
		return nil, err
	}
	if !regexp.MustCompile(`(?i)^\s*select`).MatchString(q.SQL) {
		if _, err := db.Exec(q.SQL, q.Params...); err != nil {
			return nil, err
		}
		return []byte("[]"), nil
	}
	rows, err := db.Query(q.SQL, q.Params...)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	cols, _ := rows.Columns()
	out := []map[string]any{}
	for rows.Next() {
		vals := make([]any, len(cols))
		ptrs := make([]any, len(cols))
		for i := range vals {
			ptrs[i] = &vals[i]
		}
		if err := rows.Scan(ptrs...); err != nil {
			return nil, err
		}
		m := map[string]any{}
		for i, c := range cols {
			if b, ok := vals[i].([]byte); ok {
				m[c] = string(b)
			} else {
				m[c] = vals[i]
			}
		}
		out = append(out, m)
	}
	return json.Marshal(out)
}

type config struct {
	turnstileSecret string
	ipSalt          string
	autoApprove     bool
	verifyURL       string
}

func envConfig() config {
	salt := os.Getenv("IP_SALT")
	if salt == "" {
		salt = "cligc-pilot"
	}
	return config{
		turnstileSecret: os.Getenv("TURNSTILE_SECRET_KEY"),
		ipSalt:          salt,
		autoApprove:     os.Getenv("COMMENTS_AUTO_APPROVE") == "1",
		verifyURL:       "https://challenges.cloudflare.com/turnstile/v0/siteverify",
	}
}

type app struct {
	db  *sql.DB
	cfg config
	now func() time.Time
}

func newApp(db *sql.DB, cfg config) http.Handler {
	a := &app{db: db, cfg: cfg, now: time.Now}
	mux := http.NewServeMux()
	mux.HandleFunc("/api/form", a.form)
	mux.HandleFunc("/api/comments", a.comments)
	mux.HandleFunc("/api/health", a.health)
	return mux
}

func writeJSON(w http.ResponseWriter, status int, v any, cache string) {
	w.Header().Set("content-type", "application/json; charset=utf-8")
	if cache == "" {
		cache = "no-store"
	}
	w.Header().Set("cache-control", cache)
	w.WriteHeader(status)
	_ = json.NewEncoder(w).Encode(v)
}

// reply：普通表单提交（Accept 里有 text/html、不是 JSON）跳回来源页并带上结果；fetch 提交返回 JSON。
func reply(w http.ResponseWriter, r *http.Request, ok bool, extra map[string]any, status int) {
	if strings.Contains(r.Header.Get("accept"), "text/html") && !strings.Contains(r.Header.Get("content-type"), "application/json") {
		back := r.Header.Get("referer")
		if back == "" {
			back = "/"
		}
		u, err := url.Parse(back)
		if err == nil {
			q := u.Query()
			if ok {
				q.Set("sent", "1")
			} else {
				e, _ := extra["error"].(string)
				if e == "" {
					e = "failed"
				}
				q.Set("error", e)
			}
			u.RawQuery = q.Encode()
			http.Redirect(w, r, u.String(), http.StatusSeeOther)
			return
		}
	}
	body := map[string]any{"ok": ok}
	for k, v := range extra {
		body[k] = v
	}
	if status == 0 {
		status = 200
		if !ok {
			status = 400
		}
	}
	writeJSON(w, status, body, "")
}

// readBody：JSON 或表单都收；同名字段合成「a, b」。
func readBody(r *http.Request) (map[string]string, error) {
	r.Body = http.MaxBytesReader(nil, r.Body, 1<<20)
	out := map[string]string{}
	if strings.Contains(r.Header.Get("content-type"), "application/json") {
		var m map[string]any
		if err := json.NewDecoder(r.Body).Decode(&m); err != nil {
			return nil, err
		}
		for k, v := range m {
			switch x := v.(type) {
			case string:
				out[k] = x
			case []any:
				parts := []string{}
				for _, p := range x {
					parts = append(parts, fmt.Sprint(p))
				}
				out[k] = strings.Join(parts, ", ")
			case nil:
			default:
				out[k] = fmt.Sprint(x)
			}
		}
		return out, nil
	}
	if err := r.ParseMultipartForm(1 << 20); err != nil && !errors.Is(err, http.ErrNotMultipart) {
		return nil, err
	}
	for k, vs := range r.PostForm {
		out[k] = strings.Join(vs, ", ")
	}
	return out, nil
}

func clip(s string, n int) string {
	s = strings.TrimSpace(s)
	if r := []rune(s); len(r) > n {
		return string(r[:n])
	}
	return s
}

// clientIP：Caddy 转发时放在 X-Forwarded-For 第一个；也认 CF-Connecting-IP（前面套了 Cloudflare 时）。
func clientIP(r *http.Request) string {
	if v := r.Header.Get("cf-connecting-ip"); v != "" {
		return v
	}
	if v := r.Header.Get("x-forwarded-for"); v != "" {
		return strings.TrimSpace(strings.Split(v, ",")[0])
	}
	h, _, _ := net.SplitHostPort(r.RemoteAddr)
	return h
}

func (a *app) ipHash(r *http.Request) string {
	sum := sha256.Sum256([]byte(a.cfg.ipSalt + clientIP(r)))
	return hex.EncodeToString(sum[:12])
}

func sameOrigin(r *http.Request) bool {
	o := r.Header.Get("origin")
	if o == "" {
		return true
	}
	u, err := url.Parse(o)
	return err == nil && strings.EqualFold(u.Host, r.Host)
}

func (a *app) tooMany(table, ip string, limit, minutes int) bool {
	var n int
	_ = a.db.QueryRow("SELECT COUNT(*) FROM "+table+" WHERE ip = ? AND created_at > ?", ip, a.now().Add(-time.Duration(minutes)*time.Minute).UnixMilli()).Scan(&n)
	return n >= limit
}

func (a *app) turnstileOK(body map[string]string, r *http.Request) bool {
	if a.cfg.turnstileSecret == "" {
		return true
	}
	token := body["cf-turnstile-response"]
	if token == "" {
		return false
	}
	resp, err := (&http.Client{Timeout: 10 * time.Second}).PostForm(a.cfg.verifyURL, url.Values{"secret": {a.cfg.turnstileSecret}, "response": {token}, "remoteip": {clientIP(r)}})
	if err != nil {
		return false
	}
	defer resp.Body.Close()
	var j struct {
		Success bool `json:"success"`
	}
	_ = json.NewDecoder(io.LimitReader(resp.Body, 1<<16)).Decode(&j)
	return j.Success
}

func pagePath(body map[string]string, r *http.Request) string {
	raw := body["_page"]
	if raw == "" {
		raw = r.Header.Get("referer")
	}
	u, err := url.Parse(raw)
	if err != nil {
		return ""
	}
	return clip(u.Path, 300)
}

func (a *app) form(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPost {
		writeJSON(w, 405, map[string]any{"ok": false, "error": "method"}, "")
		return
	}
	if !sameOrigin(r) {
		reply(w, r, false, map[string]any{"error": "forbidden"}, 403)
		return
	}
	body, err := readBody(r)
	if err != nil {
		reply(w, r, false, map[string]any{"error": "bad_request"}, 0)
		return
	}
	if body["_gotcha"] != "" {
		reply(w, r, true, nil, 0)
		return
	}
	ip := a.ipHash(r)
	if a.tooMany("submissions", ip, 5, 10) {
		reply(w, r, false, map[string]any{"error": "rate_limited"}, 429)
		return
	}
	if !a.turnstileOK(body, r) {
		reply(w, r, false, map[string]any{"error": "captcha"}, 0)
		return
	}
	fields := map[string]string{}
	for k, v := range body {
		if strings.HasPrefix(k, "_") || k == "cf-turnstile-response" {
			continue
		}
		if v = clip(v, 5000); v != "" {
			fields[clip(k, 60)] = v
		}
	}
	if len(fields) == 0 {
		reply(w, r, false, map[string]any{"error": "empty"}, 0)
		return
	}
	if len(fields) > 30 {
		reply(w, r, false, map[string]any{"error": "too_many_fields"}, 0)
		return
	}
	name := body["_form"]
	if name == "" {
		name = "contact"
	}
	fj, _ := json.Marshal(fields)
	if _, err := a.db.Exec("INSERT INTO submissions (form, page, fields, ip, ua, created_at) VALUES (?, ?, ?, ?, ?, ?)",
		clip(name, 40), pagePath(body, r), string(fj), ip, clip(r.UserAgent(), 200), a.now().UnixMilli()); err != nil {
		reply(w, r, false, map[string]any{"error": "server"}, 500)
		return
	}
	reply(w, r, true, nil, 0)
}

func (a *app) comments(w http.ResponseWriter, r *http.Request) {
	switch r.Method {
	case http.MethodGet, http.MethodHead:
		path := r.URL.Query().Get("path")
		if !strings.HasPrefix(path, "/") {
			writeJSON(w, 400, map[string]any{"ok": false, "error": "path"}, "")
			return
		}
		rows, err := a.db.Query("SELECT id, parent_id, name, url, content, created_at FROM comments WHERE path = ? AND status = 'approved' ORDER BY created_at ASC LIMIT 500", clip(path, 300))
		if err != nil {
			writeJSON(w, 500, map[string]any{"ok": false, "error": "server"}, "")
			return
		}
		defer rows.Close()
		list := []map[string]any{}
		for rows.Next() {
			var id, created int64
			var parent sql.NullInt64
			var name, u, content string
			if rows.Scan(&id, &parent, &name, &u, &content, &created) != nil {
				continue
			}
			c := map[string]any{"id": id, "parent_id": nil, "name": name, "url": u, "content": content, "created_at": created}
			if parent.Valid {
				c["parent_id"] = parent.Int64
			}
			list = append(list, c)
		}
		writeJSON(w, 200, map[string]any{"ok": true, "comments": list}, "public, max-age=30")
	case http.MethodPost:
		if !sameOrigin(r) {
			reply(w, r, false, map[string]any{"error": "forbidden"}, 403)
			return
		}
		body, err := readBody(r)
		if err != nil {
			reply(w, r, false, map[string]any{"error": "bad_request"}, 0)
			return
		}
		if body["_gotcha"] != "" {
			reply(w, r, true, map[string]any{"pending": true}, 0)
			return
		}
		path, name, content := clip(body["path"], 300), clip(body["name"], 60), clip(body["content"], 5000)
		if !strings.HasPrefix(path, "/") || name == "" || len([]rune(content)) < 2 {
			reply(w, r, false, map[string]any{"error": "invalid"}, 0)
			return
		}
		ip := a.ipHash(r)
		if a.tooMany("comments", ip, 3, 10) {
			reply(w, r, false, map[string]any{"error": "rate_limited"}, 429)
			return
		}
		if !a.turnstileOK(body, r) {
			reply(w, r, false, map[string]any{"error": "captcha"}, 0)
			return
		}
		u := clip(body["url"], 200)
		if !strings.HasPrefix(u, "http://") && !strings.HasPrefix(u, "https://") {
			u = ""
		}
		var parent any
		if p, err := strconv.ParseInt(body["parent_id"], 10, 64); err == nil {
			parent = p
		}
		status := "pending"
		if a.cfg.autoApprove {
			status = "approved"
		}
		if _, err := a.db.Exec("INSERT INTO comments (path, parent_id, name, email, url, content, status, ip, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
			path, parent, name, clip(body["email"], 120), u, content, status, ip, a.now().UnixMilli()); err != nil {
			reply(w, r, false, map[string]any{"error": "server"}, 500)
			return
		}
		reply(w, r, true, map[string]any{"pending": status == "pending"}, 0)
	default:
		writeJSON(w, 405, map[string]any{"ok": false, "error": "method"}, "")
	}
}

func (a *app) health(w http.ResponseWriter, r *http.Request) {
	var one int
	ok := a.db.QueryRow("SELECT 1").Scan(&one) == nil
	status := 200
	if !ok {
		status = 503
	}
	writeJSON(w, status, map[string]any{"ok": ok, "db": ok, "files": false, "turnstile": a.cfg.turnstileSecret != ""}, "")
}
