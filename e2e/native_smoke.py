#!/usr/bin/env python3
"""Native Tauri smoke test using Python's stdlib and WebKitWebDriver.

Prerequisites: Linux desktop, WebKitWebDriver, arduino-cli, installed ESP32 core.
Build with: npm run tauri build -- --debug --no-bundle
Run with: python3 e2e/native_smoke.py
Override the installed board with BANCADA_TEST_FQBN. No firmware is flashed.
App settings, projects, UI snapshots and results stay in a temporary directory.
Set BANCADA_TEST_SCREENSHOTS=1 to also capture images (requires a working GPU).
"""

import base64
import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import tempfile
import time
import urllib.error
import urllib.request


ROOT = Path(__file__).resolve().parents[1]
BINARY = Path(os.environ.get("BANCADA_TEST_BINARY", ROOT / "target/debug/bancada"))
FQBN = os.environ.get("BANCADA_TEST_FQBN", "esp32:esp32:esp32")
ELEMENT = "element-6066-11e4-a52e-4f735466cecf"


class NativeSmoke:
    def __init__(self):
        self.artifacts = Path(tempfile.mkdtemp(prefix="bancada-native-smoke-"))
        self.checks = []
        self.session = None
        self.driver = None
        self.log = None
        self.env = os.environ.copy()
        self.env.update(
            TAURI_WEBVIEW_AUTOMATION="true",
            XDG_CONFIG_HOME=str(self.artifacts / "config"),
            XDG_DATA_HOME=str(self.artifacts / "data"),
            XDG_CACHE_HOME=str(self.artifacts / "cache"),
        )
        self.projects = self.artifacts / "projects"
        self.projects.mkdir()
        subprocess.run(["git", "init", "--quiet", str(self.projects)], check=True)

    def request(self, method, path, payload=None):
        data = json.dumps(payload).encode() if payload is not None else None
        req = urllib.request.Request(
            self.url + path, data=data, method=method,
            headers={"Content-Type": "application/json"},
        )
        try:
            with urllib.request.urlopen(req, timeout=45) as response:
                result = json.load(response)
        except urllib.error.HTTPError as error:
            raise RuntimeError(error.read().decode()) from error
        value = result.get("value")
        if isinstance(value, dict) and value.get("error"):
            raise RuntimeError(str(value))
        return value

    def command(self, path, payload=None, method="POST"):
        return self.request(method, "/session/" + self.session + path, payload)

    def js(self, script, *args):
        return self.command("/execute/sync", {"script": script, "args": list(args)})

    def wait(self, predicate, description, timeout=20):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            value = predicate()
            if value:
                return value
            time.sleep(.15)
        raise AssertionError("Timed out: " + description)

    def element(self, selector, text=None):
        return self.wait(
            lambda: self.js(
                "return Array.from(document.querySelectorAll(arguments[0]))"
                ".find(e=>e.getClientRects().length && (arguments[1]===null || "
                "e.innerText.trim()===arguments[1])) || null;", selector, text,
            ), "visible element " + selector + " " + str(text),
        )

    def click(self, selector="button", text=None):
        element = self.element(selector, text)
        self.command("/element/" + element[ELEMENT] + "/click", {})

    def fill(self, selector, value):
        element = self.element(selector)
        self.js("arguments[0].scrollIntoView({block:'center'});", element)
        self.command("/element/" + element[ELEMENT] + "/click", {})
        self.command("/element/" + element[ELEMENT] + "/clear", {})
        self.command("/element/" + element[ELEMENT] + "/value", {"text": str(value)})

    def select(self, selector, value):
        self.element(selector)
        self.js(
            "const e=document.querySelector(arguments[0]);"
            "if(!Array.from(e.options).some(o=>o.value===arguments[1])) "
            "throw new Error('Option missing: '+arguments[1]);"
            "e.value=arguments[1];e.dispatchEvent(new Event('change',{bubbles:true}));",
            selector, value,
        )

    def check(self, condition, description):
        if not condition:
            raise AssertionError(description)
        self.checks.append(description)
        print("PASS " + description, flush=True)

    def text(self):
        return self.js("return document.body.innerText;")

    def start(self):
        if not BINARY.is_file():
            raise RuntimeError("Build the native application first: " + str(BINARY))
        if not shutil.which("WebKitWebDriver"):
            raise RuntimeError("WebKitWebDriver is required")
        with socket.socket() as sock:
            sock.bind(("127.0.0.1", 0))
            port = sock.getsockname()[1]
        self.url = "http://127.0.0.1:" + str(port)
        self.log = (self.artifacts / "webdriver.log").open("w")
        self.driver = subprocess.Popen(
            ["WebKitWebDriver", "--port=" + str(port), "--host=127.0.0.1"],
            env=self.env, stdout=self.log, stderr=self.log,
        )

        def ready():
            try:
                return self.request("GET", "/status")
            except OSError:
                return False

        self.wait(ready, "WebKitWebDriver startup")
        self.open_session()

    def open_session(self):
        value = self.request("POST", "/session", {
            "capabilities": {"alwaysMatch": {
                "webkitgtk:browserOptions": {"binary": str(BINARY)},
            }},
        })
        self.session = value["sessionId"]
        self.element(".brand-version")
        self.js(
            "window.__smokeErrors=[];"
            "addEventListener('error',e=>window.__smokeErrors.push(e.message));"
            "addEventListener('unhandledrejection',e=>window.__smokeErrors.push(String(e.reason)));"
        )

    def screenshot(self, name):
        (self.artifacts / (name + ".txt")).write_text(self.text())
        if os.environ.get("BANCADA_TEST_SCREENSHOTS") == "1":
            data = self.command("/screenshot", method="GET")
            (self.artifacts / (name + ".png")).write_bytes(base64.b64decode(data))

    def project_action(self, label):
        self.click(".project-btn")
        self.click('[role="menuitem"]', label)

    def tab(self, label):
        element = self.js(
            "return Array.from(document.querySelectorAll('.bottom-tabs button'))"
            ".find(b=>b.firstChild.textContent.trim()===arguments[0]);", label,
        )
        self.command("/element/" + element[ELEMENT] + "/click", {})
        self.wait(lambda: self.js(
            "return Array.from(document.querySelectorAll('.bottom-tabs button'))"
            ".some(b=>b.firstChild.textContent.trim()===arguments[0] && b.getAttribute('aria-current')==='true');",
            label,
        ), "active tab " + label)

    def editor(self, contents):
        element = self.element('.cm-content[contenteditable="true"]')
        self.command("/element/" + element[ELEMENT] + "/click", {})
        actions = [
            {"type": "keyDown", "value": "\ue009"},
            {"type": "keyDown", "value": "a"},
            {"type": "keyUp", "value": "a"},
            {"type": "keyUp", "value": "\ue009"},
        ]
        for char in contents:
            key = "\ue007" if char == "\n" else char
            actions.extend([
                {"type": "keyDown", "value": key},
                {"type": "keyUp", "value": key},
            ])
        self.command("/actions", {"actions": [{"type": "key", "id": "keyboard", "actions": actions}]})
        self.wait(lambda: self.js("return !!document.querySelector('.editor-tabs .tab-dot');"), "dirty editor tab")

    def save_editor(self):
        self.command("/actions", {"actions": [{
            "type": "key", "id": "keyboard", "actions": [
                {"type": "keyDown", "value": "\ue009"},
                {"type": "keyDown", "value": "s"},
                {"type": "keyUp", "value": "s"},
                {"type": "keyUp", "value": "\ue009"},
            ],
        }]})
        self.wait(lambda: self.js("return !document.querySelector('.editor-tabs .tab-dot');"), "saved editor tab")

    def compile(self, succeeds):
        self.tab("Build")
        self.js("document.querySelectorAll('button[title=Dismiss]').forEach(b=>b.click());")
        self.click(".console button", "Clear")
        self.click(".toolbar button", "✓ Verify")
        print("Compiling with real arduino-cli…", flush=True)
        expected = "✓ Compile OK" if succeeds else "Compile failed"
        self.wait(lambda: expected in self.text(), expected, timeout=240)
        self.wait(lambda: self.js(
            "return Array.from(document.querySelectorAll('.toolbar button'))"
            ".some(b=>b.innerText.trim()==='✓ Verify' && !b.disabled);"
        ), "build controls released")

    def run(self):
        print("Artifacts: " + str(self.artifacts), flush=True)
        self.start()
        self.check(self.js("return !!window.__TAURI_INTERNALS__;"), "real Tauri IPC is available")
        print("App URL: " + self.js("return location.href;"), flush=True)
        self.check(self.js(
            "return Array.from(document.querySelectorAll('.toolbar button'))"
            ".filter(b=>['✓ Verify','→ Flash'].includes(b.innerText.trim())).every(b=>b.disabled);"
        ), "build and flash disabled before opening a project")
        self.project_action("New project…")
        self.fill('input[placeholder="BlinkNode"]', "SmokeProject")
        self.fill(".np-row input", str(self.projects))
        self.wait(lambda: self.js(
            "return !!document.querySelector('.board-picker option[value="
            "'+JSON.stringify(arguments[0])+']');", FQBN,
        ), "installed board catalog")
        self.select(".board-picker select", FQBN)
        self.wait(lambda: self.js(
            "return Array.from(document.querySelectorAll('button'))"
            ".some(b=>b.innerText==='Create project' && !b.disabled);"
        ), "project can be created")
        self.click("button", "Create project")
        project = self.projects / "SmokeProject"
        ino = project / "SmokeProject.ino"
        self.wait(lambda: ino.is_file() and "SmokeProject" in self.js(
            "return document.querySelector('.project-name').innerText;"
        ), "created project loaded", timeout=120)
        self.check(FQBN in (project / "sketch.yaml").read_text(), "UI creates an ESP32 project with a pinned profile")
        self.check(not (project / ".git").exists(), "new project respects an existing parent Git repository")
        self.element(".editor-tabs [role=tab]")
        self.check("SmokeProject.ino" in self.text(), "new project opens its main source in the editor")
        for label in ["Build", "Serial", "Scope", "MQTT", "WS", "Web", "Assistant", "BOM", "Diagram"]:
            self.tab(label)
        self.check(True, "all nine bottom panels open")
        self.tab("Build")
        source = "// native e2e marker\nvoid setup() { Serial.begin(115200); }\nvoid loop() { delay(10); }\n"
        self.editor(source)
        self.save_editor()
        self.check(ino.read_text() == source, "editor keyboard save writes the exact source to disk")
        self.compile(True)
        self.check(True, "UI Verify compiles the project through its real ESP32 profile")
        self.screenshot("compile-success")
        self.editor(source + "\n#error BANCADA_SMOKE_EXPECTED_ERROR\n")
        self.save_editor()
        self.compile(False)
        self.check("BANCADA_SMOKE_EXPECTED_ERROR" in self.text(), "compiler failure is surfaced in the Build panel")
        self.screenshot("compile-error")
        self.editor(source)
        self.save_editor()
        self.compile(True)
        self.check(True, "editing the error and verifying again recovers successfully")
        self.tab("BOM")
        self.click('button[aria-label="Maximize panel"]')
        self.click(".bom-panel button", "Create BOM")
        self.fill(".bom-row td:nth-child(3) input", "R1")
        self.fill(".bom-row td:nth-child(4) input", "10k")
        self.click(".bom-row-expand")
        self.click(".bom-panel button", "+ Add connection")
        self.fill('.bom-wire-row input[placeholder="IO4"]', "OUT")
        self.fill('.bom-wire-row input[type="number"]', "4")
        self.click(".bom-panel button", "Save")
        self.wait(lambda: (project / "bom.yaml").is_file(), "BOM saved to disk")
        self.check("R1" in (project / "bom.yaml").read_text(), "BOM edit persists components and wiring")
        self.tab("Diagram")
        self.wait(lambda: self.js("return !!document.querySelector('svg[aria-label=\"Wiring diagram\"]');"), "Diagram refresh after saving BOM")
        self.check("R1" in self.text() and "OUT" in self.text(), "saved BOM wiring appears in the Diagram panel immediately")
        self.screenshot("wiring")
        self.click('button[aria-label="Restore panel"]')
        self.project_action("Rename project…")
        self.fill(".np-body input", "SmokeRenamed")
        self.click("button", "✎ Rename")
        renamed = self.projects / "SmokeRenamed"
        self.wait(lambda: (renamed / "SmokeRenamed.ino").is_file(), "renamed project")
        self.wait(lambda: "SmokeRenamed" in self.js("return document.querySelector('.project-name').innerText;"), "renamed project displayed")
        self.check(not project.exists() and (renamed / "SmokeRenamed.ino").read_text() == source, "rename moves the directory and main source without losing edits")
        self.project_action("Duplicate project…")
        self.click("button", "⧉ Duplicate")
        copied = self.projects / "SmokeRenamed-copy"
        self.wait(lambda: (copied / "SmokeRenamed-copy.ino").is_file(), "duplicated project", timeout=60)
        self.check((copied / "bom.yaml").is_file(), "duplicate carries source, profile and BOM into a separate project")
        self.check(self.js("return window.__smokeErrors;") == [], "no uncaught browser errors during the workflow")
        self.request("DELETE", "/session/" + self.session)
        self.session = None
        self.open_session()
        self.wait(lambda: "SmokeRenamed-copy" in self.js("return document.querySelector('.project-name').innerText;"), "last project restored on restart")
        self.check("SmokeRenamed-copy.ino" in self.text(), "restarting restores the last project and source file")
        self.tab("BOM")
        self.wait(lambda: self.js("return document.querySelector('.bom-row td:nth-child(3) input')?.value === 'R1';"), "saved BOM restored on restart")
        self.tab("Diagram")
        self.wait(lambda: self.js("return !!document.querySelector('svg[aria-label=\"Wiring diagram\"]');"), "saved wiring restored on restart")
        self.check(True, "BOM and wiring persist through duplicate and restart")
        self.screenshot("final")

    def close(self):
        if self.session:
            try:
                self.request("DELETE", "/session/" + self.session)
            except Exception as error:
                print("Session cleanup: " + str(error), flush=True)
            self.session = None
        if self.driver:
            self.driver.terminate()
            self.driver.wait(timeout=10)
        if self.log:
            self.log.close()


if __name__ == "__main__":
    smoke = NativeSmoke()
    error = None
    try:
        smoke.run()
    except Exception as failure:
        error = str(failure)
        if smoke.session:
            try:
                smoke.screenshot("failure")
            except Exception:
                pass
        raise
    finally:
        smoke.close()
        (smoke.artifacts / "results.json").write_text(json.dumps({
            "passed": smoke.checks, "error": error, "fqbn": FQBN,
        }, indent=2) + "\n")
