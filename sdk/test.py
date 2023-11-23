import survey_redirect as sr

# admin SDK
sdk = sr.ServeyRedirectSdk("http://127.0.0.1:443", "00000000000000000000")

params = {str(i): str(i) for i in range(100)}

# upload redirect routing table
sdk.put_redirect_tables([
    sr.Route(str(i), "https://www.google.com.hk/search", params)
    for i in range(10000)
])

# get redirect links
dat = sdk.get_links()
print(len(dat))
