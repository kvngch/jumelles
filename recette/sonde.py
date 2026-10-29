"""Sonde du runner : WebView2 par le protocole DevTools, dialogue de dossier par UI Automation."""

import os
import subprocess
import sys
import time
import urllib.request

import psutil
from PIL import ImageGrab
from playwright.sync_api import sync_playwright
from pywinauto import Desktop, findwindows


def ps(c):
    return subprocess.run(["powershell", "-NoProfile", "-Command", c], capture_output=True, text=True).stdout.strip()


def etat(p):
    for x in psutil.Process(p.pid).children(recursive=True):
        ligne = " ".join(x.cmdline())
        if "--type=" not in ligne:
            print("navigateur :", ligne[:1500])
    udf = os.path.join(os.environ["LOCALAPPDATA"], "fr.kvngch.jumelles", "EBWebView")
    print("DevToolsActivePort :", [os.path.join(r, f) for r, _, fs in os.walk(udf) for f in fs if f == "DevToolsActivePort"])
    print("fenetres :", [(h, findwindows.find_element(handle=h).name) for h in findwindows.find_windows(process=p.pid)])
    print(ps("Get-NetTCPConnection -State Listen | Where-Object LocalPort -ge 9000 | Select-Object LocalAddress,LocalPort,OwningProcess | Format-Table | Out-String"))


exe = sys.argv[1]
dossier = sys.argv[2]
print("session du script :", ps(f"(Get-Process -Id {os.getpid()}).SessionId"))
print("WebView2 :", ps("Get-ItemProperty 'HKLM:\\SOFTWARE\\WOW6432Node\\Microsoft\\EdgeUpdate\\Clients\\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}' -Name pv -ErrorAction SilentlyContinue | Select-Object -ExpandProperty pv"))
print("politiques :", ps("Get-ChildItem -Recurse HKLM:\\SOFTWARE\\Policies\\Microsoft\\Edge -ErrorAction SilentlyContinue | Out-String"))

for essai, env in (
    ("variable", {"WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS": "--remote-debugging-port=9222"}),
    ("variable et origines", {"WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS": "--remote-debugging-port=9222 --remote-allow-origins=*"}),
    ("port 0", {"WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS": "--remote-debugging-port=0"}),
    ("registre", {}),
):
    print(f"\n=== essai par {essai}")
    if essai == "registre":
        print(ps("New-Item -Force 'HKCU:\\Software\\Policies\\Microsoft\\Edge\\WebView2\\AdditionalBrowserArguments' | Out-Null; "
                 "Set-ItemProperty 'HKCU:\\Software\\Policies\\Microsoft\\Edge\\WebView2\\AdditionalBrowserArguments' -Name 'jumelles.exe' -Value '--remote-debugging-port=9222'"))
    p = subprocess.Popen([exe], env=dict(os.environ, **env))
    time.sleep(15)
    print("vivant :", p.poll() is None, "session :", ps(f"(Get-Process -Id {p.pid}).SessionId"))
    etat(p)
    ImageGrab.grab().save(f"sonde-ecran-{essai}.png")
    try:
        print(urllib.request.urlopen("http://127.0.0.1:9222/json/version", timeout=5).read().decode()[:300])
        ok = True
    except Exception as e:
        print("DevTools :", e)
        ok = False
    if not ok:
        p.kill()
        time.sleep(2)
        continue

    with sync_playwright() as pw:
        b = pw.chromium.connect_over_cdp("http://127.0.0.1:9222")
        page = b.contexts[0].pages[0]
        print("url", page.url)
        print("langue", page.evaluate("navigator.language"), page.evaluate("navigator.languages"))
        print("taille", page.evaluate("[innerWidth, innerHeight, devicePixelRatio]"))
        page.screenshot(path="sonde-accueil.png")

        page.click("[data-action=ajouter]")
        hs = []
        for _ in range(50):
            hs = findwindows.find_windows(process=p.pid, class_name="#32770")
            if hs:
                break
            time.sleep(0.2)
        print("dialogues", hs)
        d = Desktop(backend="uia").window(handle=hs[0])
        d.wait("ready", timeout=10)
        print("titre", d.window_text())
        for e in d.descendants():
            info = e.element_info
            if info.control_type in ("Edit", "Button", "ComboBox"):
                print(" ", info.control_type, repr(info.name), info.automation_id)
        ImageGrab.grab().save("sonde-dialogue.png")
        d.child_window(auto_id="1152", control_type="Edit").set_edit_text(dossier)
        d.child_window(auto_id="1", control_type="Button").invoke()
        time.sleep(2)
        print("encore ouvert", bool(findwindows.find_windows(process=p.pid, class_name="#32770")))
        print("dossiers", page.evaluate("etat.dossiers"))

        page.evaluate(
            "p => window.__TAURI__.event.emit('tauri://drag-drop', {paths: [p], position: {x: 10, y: 10}})",
            dossier + "\\sous",
        )
        time.sleep(1)
        print("apres depot", page.evaluate("etat.dossiers"))

        page.add_init_script("Object.defineProperty(Navigator.prototype, 'language', {get: () => 'fr-FR'})")
        page.reload()
        page.wait_for_selector(".accueil")
        print("apres rechargement", page.evaluate("[navigator.language, T.code, etat.dossiers]"))
        page.screenshot(path="sonde-fr.png")
        b.close()
    p.kill()
    break
