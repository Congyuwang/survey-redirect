from typing import Dict as _Dict, List as _List, Tuple as _Tuple, Callable as _Callable
import requests as _requests
import json as _json
from urllib import parse as _parse
from dataclasses import dataclass as _dataclass, asdict as _asdict
from io import BytesIO as _BytesIO
from tqdm import tqdm as _tqdm


__all__ = ["Route", "ServeyRedirectSdk"]


_CHUNK_SIZE = 32 * 1024


@_dataclass
class Route:
    id: str
    url: str

    def __init__(self, id: str, url: str, params: _Dict[str, str]):
        self.id = id
        # parse url
        url_parts = _parse.urlparse(url)
        # parse params
        this_params = _parse.parse_qsl(url_parts.query)
        # add params
        for key, value in params.items():
            this_params.append((key, value))
        # rebuild url
        self.url = _parse.urlunparse((
            url_parts.scheme,
            url_parts.netloc,
            url_parts.path,
            url_parts.params,
            _parse.urlencode(this_params),
            url_parts.fragment
        ))


class _ReaderWrapper(object):
    def __init__(self, callback: _Callable[[int], object], stream, length):
        self.callback = callback
        self.stream = stream
        self.length = length

    def read(self, __len: int = -1) -> bytes:
        data = self.stream.read(__len)
        self.callback(len(data))
        return data

    def __len__(self):
        return self.length


class ServeyRedirectSdk:
    def __init__(self, server_url: str, admin_token: str):
        """
        Args:
            server_url (str): The URL of the redirect server.
            admin_token (str): The admin token of the redirect server.
        """
        self.server_url = server_url
        self.admin_token = admin_token

    def get_links(self) -> _Dict[str, str]:
        """Get links from server.

        Returns:
            Dict[str, str]: A mapping from user ID to their survey links.
        """
        url = self.server_url + "/admin/get_links"
        headers = {"Authorization": self.admin_token}
        response = _requests.get(url, stream=True, headers=headers)
        data = bytearray()
        total_size = int(response.headers.get('content-length', 0))
        with self.__progress_bar(desc="Downloading", total=total_size) as t:
            for chunk in response.iter_content(_CHUNK_SIZE):
                if chunk:
                    data.extend(chunk)
                    t.update(len(chunk))
        response.raise_for_status()
        return _json.loads(data)

    def put_redirect_tables(self, table: _List[Route]) -> _Tuple[int, str]:
        """Put redirect table to server.

        Replaces the existing redirect table with the given one
        (i.e., delete the old table, and put the new table).

        Args:
            table (List[Route]): The redirect table to be put.

        Returns:
            Tuple[int, str]: The status code and response text.
            (200, "success") if success. Raise exception otherwise.
        """
        # Check input
        self.__check_table(table)

        # Send request
        url = self.server_url + "/admin/routing_table"
        headers = {"Content-type": "application/json", "Authorization": self.admin_token}
        data = _json.dumps([_asdict(dat) for dat in table]).encode("utf-8")
        with self.__progress_bar(desc="Uploading", total=len(data)) as t:
            reader_wrapper = _ReaderWrapper(t.update, _BytesIO(data), len(data))
            response = _requests.put(url, headers=headers, data=reader_wrapper)
            response.raise_for_status()
            return (response.status_code, response.text)

    def __check_table(self, table: _List[Route]):
        if not isinstance(table, list):
            raise Exception("Not a list")
        if len(table) == 0:
            raise Exception("Empty table")
        for route in table:
            if not isinstance(route, Route):
                raise Exception("Not a Route object")

    def __progress_bar(self, desc: str, total: int):
        return _tqdm(desc=desc, total=total, unit="B", unit_scale=True, unit_divisor=1024)
