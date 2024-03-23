import requests
import atexit
import shutil
import os
import sys
import time
from typing import List, Tuple, Dict
from subprocess import Popen
from urllib.parse import urlparse, parse_qs
import sdk.survey_redirect as sr


def print_green(msg):
    OKGREEN = '\033[32m'
    ENDC = '\033[0m'
    print(f"{OKGREEN}{msg}{ENDC}")


def get_code_from_url(url):
    return parse_qs(urlparse(url).query)["code"][0]


def test_redirects(links_table: Dict[str, str], test_cases: List[Tuple[str, str, Dict[str, str]]]):
    for id, url, params in test_cases:
        r = requests.get(links_table[id], allow_redirects=False)
        code = get_code_from_url(links_table[id])
        assert(r.status_code == 303)
        location = urlparse(r.headers["Location"])
        assert(location.hostname == urlparse(url).hostname)
        assert(location.path == "/")
        query = parse_qs(location.query)
        for key, value in params.items():
            assert(query[key][0] == value)
        assert(query["externalUserId"][0] == code)


def build_server():
    compile = Popen(["cargo", "build", "--release"])
    compile.wait()


def launch_server() -> Popen:
    server = Popen(["cargo", "run", "--release"])
    # wait for server to start
    time.sleep(1)
    print_green("Server started")
    return server


# Start the server
build_server()
server = launch_server()


# Kill the server at exit
def cleanup():
    server.terminate()
    shutil.rmtree("./db", ignore_errors=True)
    if os.path.exists("./survey_redirect.log"):
        os.remove("./survey_redirect.log")

atexit.register(cleanup)


# Initialize the SDK

TEST_URL = 'http://127.0.0.1:6688'
ADMIN_TOKEN = '00000000000000000000'
sdk = sr.ServeyRedirectSdk(TEST_URL, ADMIN_TOKEN)


# Test basic puts

basic_test_cases = [
    ("user0", "http://url_for_user0.com", {"_id": "user0_id"}),
    ("user1", "http://url_for_user1.com", {"_id": "user1_id"}),
    ("user2", "http://url_for_user2.com", {"_id": "user2_id"}),
]
sdk.put_redirect_tables([sr.Route(id, url, params) for id, url, params in basic_test_cases])
links = sdk.get_links()
assert(set(links.keys()) == {"user0", "user1", "user2"})
print_green("Basic puts passed!")


# Test redirect functionality

test_redirects(links, basic_test_cases)
print_green("Redirect functionality passed!")


# Test replacing redirect tables

sdk.put_redirect_tables([
    sr.Route(uid="user0", url="http://url_for_user0.com", params={"_id": "user0", "new_param": "new_value"}),
])
# assert links not changed
new_links = sdk.get_links()
assert(list(new_links.keys()) == ["user0"])
assert(new_links["user0"] == links["user0"])
test_redirects(new_links, [("user0", "http://url_for_user0.com", {"_id": "user0", "new_param": "new_value"})])
print_green("Replace redirect tables passed!")


# Testing restoring links does not change code

sdk.put_redirect_tables([sr.Route(id, url, params) for id, url, params in basic_test_cases])
assert(links == sdk.get_links())
test_redirects(links, basic_test_cases)
print_green("Restore links passed!")


# Test partial update (patch) of redirect tables

sdk.patch_redirect_tables([
    sr.Route(uid="user0", url="http://url_for_user0.com", params={"_id": "user0", "new_param": "new_value"}),
    sr.Route(uid="user4", url="http://url_for_user4.com", params={"_id": "user4"}),
])
new_links = sdk.get_links()
for id in ["user0", "user1", "user2"]:
    assert(links[id] == new_links[id])
links = new_links
# assert redirect functionality after update
updated_test_cases = [
    ("user0", "http://url_for_user0.com", {"_id": "user0", "new_param": "new_value"}),
    ("user1", "http://url_for_user1.com", {"_id": "user1_id"}),
    ("user2", "http://url_for_user2.com", {"_id": "user2_id"}),
    ("user4", "http://url_for_user4.com", {"_id": "user4"}),
]
test_redirects(links, updated_test_cases)
print_green("Partial update of redirect tables passed!")


# Test server restart does not change links

server.terminate()
server = launch_server()
assert(links == sdk.get_links())
test_redirects(links, updated_test_cases)
print_green("Server restart does not change links passed!")


# done!
print_green("All tests passed!")
