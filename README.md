# Python SDK Documentation

This is a simple server that provides a Python SDK for the survey redirect service.

## Initializing the Admin SDK Object

Two parameters are provided:
1. Server address (https://your-redirect-server.com)
2. Server key (defined according to configuration)

```python
import survey_redirect as sr

sdk = sr.ServeyRedirectSdk(
    "https://your-redirect-server.com",
    "your-admin-token-here"
)
```

## Admin APIs

### Delete and Rewrite Redirect Table (PUT)

This API will replace the existing redirect table (delete the old table and write a new one).

The API requires one parameter (list):
Each item in the list is an `sr.Route` object,
Each object has three values:
1. User ID
2. The user's redirect URL
3. The user's URL parameter dictionary (ID not required)

Subsequently, this user will be redirected to `URL?ExternalId=User ID&Param1=Value1&Param2=Value2...&ParamN=ValueN`.

```python
params1 = {
    "_ei8h": "10",
    "_t7xn": "20",
    "_YWz5": "30",
    "_L8OD": "40",
    "_fIFb": "50",
    "_bsVS": "60",
    "return_rate": "20"
}

params2 = {
    "_ei8h": "-10",
    "_t7xn": "-20",
    "_YWz5": "-30",
    "_L8OD": "-40",
    "_fIFb": "-50",
    "_bsVS": "-60",
}

sdk.put_redirect_tables([
    # Survey test
    sr.Route(
        "161616161616",
        "https://www.surveyplus.cn/lite/5382278238929920",
        params1
    ),
    # Survey test
    sr.Route(
        "161616161617",
        "https://www.surveyplus.cn/lite/5382278238929920",
        params2
    ),
    # Survey test
    sr.Route(
        "161616161618",
        "https://www.surveyplus.cn/lite/5731702823496704",
        params1
    ),
])
```

### Partial Update Redirect Table (PATCH)

This API will partially update the existing redirect table (do not delete the old table, incrementally write a new table).

The parameters and usage are the same as PUT.

```python
sdk.patch_redirect_tables([...])
```

### Get All User Redirect Links

```python
print(sdk.get_links())
```

```json
{
  "1": "https://your-redirect-server.com/api?code=sHFFnisbviqsDjWko53c",
  "2": "https://your-redirect-server.com/api?code=uQhUBoyGFHLyXvNa32qE",
  "3": "https://your-redirect-server.com/api?code=dUoAypGIKc7EBXkaSNTA",
  "4": "https://your-redirect-server.com/api?code=q0FkK5kFM0Be37q1XTEJ",
  "12345": "https://your-redirect-server.com/api?code=0i6MLfNxg64vROvcXzYZ",
  "67890": "https://your-redirect-server.com/api?code=iqTvlIHi3vF1JDjRWOwi"
}
```

### Get All User ID-CODE Correspondence Table

```python
print(sdk.get_codes())
```

```json
{
  "1": "sHFFnisbviqsDjWko53cUgJdfW",
  "2": "uQhUBoyGFHLyXvNa32qE52vigi",
  "3": "dUoAypGIKc7EBXkaSNTANB1Y2y",
  "4": "q0FkK5kFM0Be37q1XTEJy8NDN3",
  "12345": "0i6MLfNxg64vROvcXzYZNy1tZ7",
  "67890": "iqTvlIHi3vF1JDjRWOwib1M8Gn"
}
```
