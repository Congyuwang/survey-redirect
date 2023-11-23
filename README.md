# A Simple Redirect Server

Redirect user based on the provided ID.

## How to use

```python
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
```

Output:
```json
{
  "1": "http://127.0.0.1:443/api?id=1&code=PbpDFBXaippx5g8GQyJtfOZxJEE8aB8OOWfQK8uSEAtvN4V6u9UFgLT6XVBnHjCh",
  "2": "http://127.0.0.1:443/api?id=2&code=Ac6oWEaYG1JGWD9HDzfyOogJVBkTNSOjoultNIMMvY2KxeyGTtb3NpO84U7tlHp8"
}
```

The first link will redirect to
`https://www.google.com.hk/search?q=Gaza`
and the second link will redirect to
`https://www.google.com.hk/search?=Israel`.
