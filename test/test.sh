./target/debug/envd test/server.yml &
ENV_PID=$!
sleep 2
# Test health
curl -s http://localhost:7878/health
# Test auth failure
curl -s -o /dev/null -w "\n%{http_code}" http://localhost:7878/projects
# Test create project
curl -s -o /dev/null -w "\n%{http_code}" -X POST \
  -H "Authorization: qwqw" \
  -H "Content-Type: application/json" \
  -d '{"name":"myapp"}' \
  http://localhost:7878/projects
# Test list projects
curl -s -H "Authorization: qwqw" http://localhost:7878/projects
# Test set envs
curl -s -o /dev/null -w "\n%{http_code}" -X POST \
  -H "Authorization: qwqw" \
  -H "Content-Type: application/json" \
  -d '{"envs":{"DATABASE_URL":"postgres://localhost/myapp","API_KEY":"secret"}}' \
  http://localhost:7878/projects/myapp/envs
# Test get envs
echo "--- envs ---"
curl -s -H "Authorization: qwqw" http://localhost:7878/projects/myapp/envs
# Test get single env via client
echo "--- client get ---"
mkdir -p ~/.config/envd
cat > ~/.config/envd/client.yml << 'EOF'
config:
  endpoint: http://localhost:7878
  token:    qwqw
projects:
  myapp:   /home/mod/Dev/Rust/envd/
EOF
./target/debug/enve get --project myapp
# Cleanup
kill $ENV_PID 2>/dev/null
wait $ENV_PID 2>/dev/null
