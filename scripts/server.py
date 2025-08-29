#!/usr/bin/env python3
from http.server import HTTPServer, SimpleHTTPRequestHandler, test
import sys
import os

scripts_dir = os.path.dirname(__file__)
project_dir = os.path.abspath(os.path.join(scripts_dir, '..'))
web_dir = os.path.join(project_dir, 'web')

class RequestHandler(SimpleHTTPRequestHandler):
    def end_headers(self):
        self.send_header('Cross-Origin-Opener-Policy', 'same-origin')
        self.send_header('Cross-Origin-Embedder-Policy', 'require-corp')
        SimpleHTTPRequestHandler.end_headers(self)

if __name__ == '__main__':
    os.chdir(web_dir)
    test(RequestHandler, HTTPServer, port=int(sys.argv[1]) if len(sys.argv) > 1 else 8765)
