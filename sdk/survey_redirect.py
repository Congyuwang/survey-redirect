from typing import Dict, List, Tuple
import requests
import json
from dataclasses import dataclass, asdict


@dataclass
class Route:
    id: str
    url: str
    params: Dict[str, str]

    def __init__(self, id: str, url: str, params: Dict[str, str]):
        self.id = id
        self.url = url
        self.params = params



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
        response = requests.get(url, headers=headers)
        if response.status_code == 200:
            return response.json()
        else:
            raise Exception(response.status_code, response.text)

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
        headers = {"Authorization": self.admin_token}
        response = requests.put(url, headers=headers, json=[asdict(dat) for dat in table])
        if response.status_code == 200:
            return (response.status_code, response.text)
        else:
            raise Exception(response.status_code, response.text)

    def patch_redirect_tables(self, table: List[Route]) -> Tuple[int, str]:
        """Patch redirect table to server.

        Overwrite existing data, and insert new route if id not exist.

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
        headers = {"Authorization": self.admin_token}
        response = requests.patch(url, headers=headers, json=[asdict(dat) for dat in table])
        if response.status_code == 200:
            return (response.status_code, response.text)
        else:
            raise Exception(response.status_code, response.text)

    def __check_table(self, table: List[Route]):
        if not isinstance(table, list):
            raise Exception("Not a list")
        if len(table) == 0:
            raise Exception("Empty table")
        for route in table:
            if not isinstance(route, Route):
                raise Exception("Not a Route object")
