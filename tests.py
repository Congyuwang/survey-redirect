import requests
import atexit
import shutil
import os
import sys
import time
from typing import List, Tuple, Dict
from subprocess import Popen, PIPE
from urllib.parse import urlparse, parse_qs
import sdk.survey_redirect as sr


TEST_URL = 'https://localhost:6689'
ADMIN_TOKEN = '00000000000000000000'
CERT_PATH = './dev_certs/localhost.crt'


def print_green(msg):
    OKGREEN = '\033[32m'
    ENDC = '\033[0m'
    print(f"{OKGREEN}{msg}{ENDC}")


def get_code_from_url(url):
    return parse_qs(urlparse(url).query)["code"][0]


def test_redirects(links_table: Dict[str, str], test_cases: List[Tuple[str, str, Dict[str, str]]]):
    for id, url, params in test_cases:
        r = requests.get(links_table[id], allow_redirects=False, verify=CERT_PATH)
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



def gen_certs():
    # refresh certs: run `./gen_certs.sh`
    generate = Popen(["/bin/bash", "./gen_certs.sh"])
    generate.wait()


def launch_server() -> Popen:
    env = os.environ.copy()
    env["RUST_LOG"] = f"trace"
    server = Popen(["cargo", "run", "--release"], env=env)
    # wait for server to start
    time.sleep(1)
    print_green("Server started")
    return server


# Start the server
build_server()
gen_certs()
server = launch_server()


# Kill the server at exit
def cleanup():
    server.terminate()
    shutil.rmtree("./db", ignore_errors=True)
    if os.path.exists("./survey_redirect.log"):
        os.remove("./survey_redirect.log")

atexit.register(cleanup)

def tests(check_codes = False):
    global server
    # Initialize the SDK
    sdk = sr.ServeyRedirectSdk(TEST_URL, ADMIN_TOKEN)

    # Test basic puts
    basic_test_cases = [
        ("user0", "http://url_for_user0.com", {"_id": "user0_id"}),
        ("user1", "http://url_for_user1.com", {"_id": "user1_id"}),
        ("user2", "http://url_for_user2.com", {"_id": "user2_id"}),
    ]
    sdk.put_redirect_tables([sr.Route(id, url, params) for id, url, params in basic_test_cases], verify=CERT_PATH)
    links = sdk.get_links(verify=CERT_PATH)
    assert(set(links.keys()) == {"user0", "user1", "user2"})
    if check_codes:
        assert sdk.get_codes(verify=CERT_PATH) == {id: get_code_from_url(links[id]) for id in links.keys()}
    print_green("Basic puts passed!")

    # Test redirect functionality
    test_redirects(links, basic_test_cases)
    print_green("Redirect functionality passed!")

    # Test replacing redirect tables
    sdk.put_redirect_tables([
        sr.Route(uid="user0", url="http://url_for_user0.com", params={"_id": "user0", "new_param": "new_value"}),
    ], verify=CERT_PATH)
    # assert links not changed
    new_links = sdk.get_links(verify=CERT_PATH)
    assert(list(new_links.keys()) == ["user0"])
    assert(new_links["user0"] == links["user0"])
    test_redirects(new_links, [("user0", "http://url_for_user0.com", {"_id": "user0", "new_param": "new_value"})])
    print_green("Replace redirect tables passed!")

    # Testing restoring links does not change code
    sdk.put_redirect_tables([sr.Route(id, url, params) for id, url, params in basic_test_cases], verify=CERT_PATH)
    assert(links == sdk.get_links(verify=CERT_PATH))
    test_redirects(links, basic_test_cases)
    print_green("Restore links passed!")

    # Test partial update (patch) of redirect tables
    sdk.patch_redirect_tables([
        sr.Route(uid="user0", url="http://url_for_user0.com", params={"_id": "user0", "new_param": "new_value"}),
        sr.Route(uid="user4", url="http://url_for_user4.com", params={"_id": "user4"}),
    ], verify=CERT_PATH)
    new_links = sdk.get_links(verify=CERT_PATH)
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
    assert(links == sdk.get_links(verify=CERT_PATH))
    test_redirects(links, updated_test_cases)
    print_green("Server restart does not change links passed!")

    # done!
    print_green("All tests passed!")


# Run the tests
tests(True)
# Update the certs
gen_certs()
# wait for server restart
time.sleep(1)
# Server should restart
tests()
# Update the certs
gen_certs()
# wait for server restart
time.sleep(1)
# Server should restart
tests()
