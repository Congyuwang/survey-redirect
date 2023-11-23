from typing import Dict, List, Tuple, Callable
import requests
import json
from dataclasses import dataclass, asdict
from io import BytesIO
from tqdm import tqdm


CHUNK_SIZE = 32 * 1024


@dataclass
class Route:
    id: str
    url: str
    params: Dict[str, str]

    def __init__(self, id: str, url: str, params: Dict[str, str]):
        self.id = id
        self.url = url
        self.params = params


class ReaderWrapper(object):
    def __init__(self, callback: Callable[[int], object], stream, length):
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

    def get_links(self) -> Dict[str, str]:
        """Get links from server.

        Returns:
            Dict[str, str]: A mapping from user ID to their survey links.
        """
        url = self.server_url + "/admin/get_links"
        headers = {"Authorization": self.admin_token}
        response = requests.get(url, stream=True, headers=headers)
        data = bytearray()
        total_size = int(response.headers.get('content-length', 0))
        with tqdm(desc=f"Downloading", total=total_size, unit="B", unit_scale=True, unit_divisor=1024) as t:
            for chunk in response.iter_content(CHUNK_SIZE):
                if chunk:
                    data.extend(chunk)
                    t.update(len(chunk))
        response.raise_for_status()
        return json.loads(data)

    def put_redirect_tables(self, table: List[Route]) -> Tuple[int, str]:
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
        data = json.dumps([asdict(dat) for dat in table]).encode("utf-8")
        with tqdm(desc=f"Uploading", total=len(data), unit="B", unit_scale=True, unit_divisor=1024) as t:
            reader_wrapper = ReaderWrapper(t.update, BytesIO(data), len(data))
            response = requests.put(url, headers=headers, data=reader_wrapper)
            response.raise_for_status()
            return (response.status_code, response.text)

    def __check_table(self, table: List[Route]):
        if not isinstance(table, list):
            raise Exception("Not a list")
        if len(table) == 0:
            raise Exception("Empty table")
        for route in table:
            if not isinstance(route, Route):
                raise Exception("Not a Route object")
