import survey_redirect as sr

# admin SDK
sdk = sr.ServeyRedirectSdk("http://127.0.0.1:443", "00000000000000000000")

# upload redirect routing table
sdk.put_redirect_tables([
    sr.Route("1", "https://www.google.com.hk/search", {"q": "Gaza"}),
    sr.Route("2", "https://www.google.com.hk/search", {"q": "Israel"})
])

# get redirect links
print(sdk.get_links())
