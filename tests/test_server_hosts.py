import http.client
import re
import socket
import tempfile
import threading
import time
import unittest

import genemapper_open.server as S


def free_port():
    s = socket.socket(); s.bind(('127.0.0.1', 0)); p = s.getsockname()[1]; s.close(); return p


class HostAllowList(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.tmp = tempfile.TemporaryDirectory()
        cls.port = free_port()
        threading.Thread(target=S.serve, kwargs={
            'port': cls.port, 'projects_dir': cls.tmp.name + '/p', 'security_dir': cls.tmp.name + '/s',
            'host': '127.0.0.1', 'allowed_hosts': ['localhost', 'Lab.Example']}, daemon=True).start()
        for _ in range(100):
            try:
                cls.req('GET', '/api/health', {'Host': f'127.0.0.1:{cls.port}'}); break
            except OSError:
                time.sleep(0.05)

    @classmethod
    def tearDownClass(cls):
        cls.tmp.cleanup()

    @classmethod
    def req(cls, method, path, headers, body=None):
        c = http.client.HTTPConnection('127.0.0.1', cls.port, timeout=5)
        c.request(method, path, body=body, headers=headers)
        r = c.getresponse(); data = r.read(); c.close()
        return r.status, data

    def test_get_host_whitelist(self):
        for host, want in [(f'127.0.0.1:{self.port}', 200), ('localhost:3001', 200), ('lab.example', 200),
                           ('LAB.EXAMPLE:443', 200), ('evil.com:8765', 403), ('localhost.evil.com', 403),
                           ('127.0.0.1:1', 403)]:
            self.assertEqual(self.req('GET', '/api/health', {'Host': host})[0], want, host)

    def test_post_origin_must_match_host(self):
        _, page = self.req('GET', '/idx/', {'Host': 'lab.example'})
        token = re.search(rb'[A-Za-z0-9_-]{32}', page).group(0).decode()
        def post(origin):
            h = {'Host': 'lab.example', 'X-Session-Token': token, 'Content-Type': 'application/json'}
            if origin: h['Origin'] = origin
            return self.req('POST', '/api/analysis-method', h, b'{"action":"factory"}')[0]
        self.assertEqual(post('http://lab.example'), 200)
        self.assertEqual(post('https://lab.example'), 200)
        self.assertEqual(post('http://evil.com'), 403)


class DefaultIsLoopbackOnly(unittest.TestCase):
    def test_without_allow_list_other_hosts_rejected(self):
        tmp = tempfile.TemporaryDirectory(); port = free_port()
        threading.Thread(target=S.serve, kwargs={'port': port, 'projects_dir': tmp.name + '/p',
                         'security_dir': tmp.name + '/s'}, daemon=True).start()
        for _ in range(100):
            try:
                c = http.client.HTTPConnection('127.0.0.1', port, timeout=5)
                c.request('GET', '/api/health', headers={'Host': f'127.0.0.1:{port}'}); ok = c.getresponse().status; break
            except OSError:
                time.sleep(0.05)
        self.assertEqual(ok, 200)
        c = http.client.HTTPConnection('127.0.0.1', port, timeout=5)
        c.request('GET', '/api/health', headers={'Host': 'localhost:%d' % port})
        self.assertEqual(c.getresponse().status, 403)
        tmp.cleanup()


if __name__ == '__main__':
    unittest.main()
