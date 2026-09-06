// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: Apache-2.0

package main

import (
	"bytes"
	"crypto/tls"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"os"
	"time"
)

func main() {
	base := env("BASE", "https://127.0.0.1:8080")
	project := env("PROJECT", "projects/demo")
	client := &http.Client{
		Timeout: 30 * time.Second,
		Transport: &http.Transport{
			TLSClientConfig: &tls.Config{InsecureSkipVerify: true}, // lab self-signed
		},
	}
	topic := project + "/topics/matrix-go"
	sub := project + "/subscriptions/matrix-go-sub"

	_ = do(client, "PUT", base+"/v1/"+topic, map[string]any{"labels": map[string]string{"lane": "go"}})
	_ = do(client, "PUT", base+"/v1/"+sub, map[string]any{"topic": topic, "ackDeadlineSeconds": 20})
	pub := mustMap(do(client, "POST", base+"/v1/"+topic+":publish", map[string]any{
		"messages": []map[string]any{{
			"data":       base64.StdEncoding.EncodeToString([]byte("go")),
			"attributes": map[string]string{"source": "matrix-go"},
		}},
	}))
	ids, _ := pub["messageIds"].([]any)
	if len(ids) == 0 {
		panic(fmt.Sprintf("publish failed: %v", pub))
	}
	pulled := mustMap(do(client, "POST", base+"/v1/"+sub+":pull", map[string]any{"maxMessages": 5}))
	msgs, _ := pulled["receivedMessages"].([]any)
	if len(msgs) == 0 {
		panic("no messages")
	}
	ackIDs := make([]string, 0, len(msgs))
	for _, m := range msgs {
		mm := m.(map[string]any)
		ackIDs = append(ackIDs, mm["ackId"].(string))
	}
	_ = do(client, "POST", base+"/v1/"+sub+":acknowledge", map[string]any{"ackIds": ackIDs})
	fmt.Println("go-rest ok", ids[0])
}

func env(k, d string) string {
	if v := os.Getenv(k); v != "" {
		return v
	}
	return d
}

func do(c *http.Client, method, url string, body any) map[string]any {
	var rdr io.Reader
	if body != nil {
		b, _ := json.Marshal(body)
		rdr = bytes.NewReader(b)
	}
	req, err := http.NewRequest(method, url, rdr)
	if err != nil {
		panic(err)
	}
	req.Header.Set("content-type", "application/json")
	resp, err := c.Do(req)
	if err != nil {
		panic(err)
	}
	defer resp.Body.Close()
	raw, _ := io.ReadAll(resp.Body)
	if resp.StatusCode >= 300 && resp.StatusCode != 409 {
		panic(fmt.Sprintf("%s %s → %d %s", method, url, resp.StatusCode, string(raw)))
	}
	if len(raw) == 0 {
		return map[string]any{}
	}
	var out map[string]any
	if err := json.Unmarshal(raw, &out); err != nil {
		panic(err)
	}
	return out
}

func mustMap(v map[string]any) map[string]any { return v }
