"""Prepares a TEST OpenTrace Server for the desktop self-test with a laboratory profile.

Creates, submits (as "Casework Analyst") and approves (as "Lab Director", who must be in "OpenTrace Approvers")
the shipped MiniFiler method, the CE_G5_HID_GS500 size standard, and either the synthetic kit printed by
`cargo run --example synthetic_kit_store` (with bins and stutter) or a minimal placeholder kit; then a lab profile
pinning them. Prints "<profile>:<version>" for OPENTRACE_SELFTEST_PROFILE. Never point it at a production server.

  python3 setup_lab_profile.py URL PASSWORD resources/data/shipped_methods.json [synthetic_kit.json]
"""
import json
import sys
import urllib.parse
import urllib.request

base, pw, shipped = sys.argv[1], sys.argv[2], sys.argv[3]
kit_json = json.load(open(sys.argv[4])) if len(sys.argv) > 4 else None


def call(method, path, body=None, token=None):
    headers = {"content-type": "application/json"}
    if token:
        headers["authorization"] = "Bearer " + token
    req = urllib.request.Request(base + path, method=method, data=None if body is None else json.dumps(body).encode(), headers=headers)
    with urllib.request.urlopen(req) as r:
        return json.load(r)


def login(user):
    return call("POST", "/api/v1/auth/login", {"username": user, "password": pw})["token"]


author, director = login("Casework Analyst"), login("Lab Director")


def approved(kind, name, content):
    p = "/api/v1/standards/%s/%s" % (kind, urllib.parse.quote(name, safe=""))
    v = call("POST", p, {"content": content}, author)["version"]
    call("POST", "%s/%d/submit" % (p, v), {}, author)
    assert call("POST", "%s/%d/approve" % (p, v), {}, director)["status"] == "approved"
    return {"kind": kind, "name": name, "version": v}


method = json.load(open(shipped))["methods"][0]
members = [
    approved("analysis_method", method["name"], method),
    approved("size_standard", "CE_G5_HID_GS500", {"name": "CE_G5_HID_GS500", "dye": "orange", "sizes": [75, 100, 139, 150, 160, 200, 300, 340, 350, 400, 450], "description": ""}),
]
if kit_json:
    kit = kit_json["kit"]["name"]
    members += [approved("kit", kit, kit_json["kit"]), approved("bin_set", kit_json["bin_set_name"], kit_json["bin_set"]), approved("stutter", kit, kit_json["stutter"])]
    profile = "Lab Synthetic"
else:
    members += [approved("kit", "Kit A", {"name": "Kit A", "markers": []}), approved("bin_set", "Kit A · Bins", {"kit": "Kit A", "name": "Bins"})]
    profile = "Lab GF"
print("%s:%d" % (profile, approved("lab_profile", profile, {"members": members})["version"]))
