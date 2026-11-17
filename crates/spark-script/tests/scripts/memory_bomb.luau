local t = {}
function start()
    local ok, err = pcall(function()
        local s = string.rep('x', 1024 * 1024)
        for i = 1, 100000 do t[i] = s .. i end
    end)
    t = nil
    assert(not ok, 'memory bomb was not stopped')
    assert(string.find(tostring(err), 'memory', 1, true), tostring(err))
    quit()
end
