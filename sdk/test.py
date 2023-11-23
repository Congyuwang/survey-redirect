import survey_redirect as sr


sdk = sr.ServeyRedirectSdk("http://127.0.0.1:443", "00000000000000000000")

print(sdk.get_links())

print(sdk.put_redirect_tables([sr.Route("1", "https://www.google.com.hk/search", {"q": "Israel"})]))

print(sdk.get_links())

print(sdk.put_redirect_tables([
    sr.Route("1", "https://www.google.com.hk/search", {"q": "Gaza"}),
    sr.Route("2", "https://www.google.com.hk/search", {"q": "Israel"})
]))

print(sdk.get_links())
