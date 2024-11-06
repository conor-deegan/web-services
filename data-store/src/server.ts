import net from 'net';
import fs from 'fs/promises';
import path from 'path';

// Define the data type
type Data = {
    id: number;
    name: string;
    description: string;
};

// Load data
async function loadData(table: string): Promise<Data[]> {
    try {
        const dataFilePath = path.join(__dirname, '..', `data/${table}.txt`);
        const data = await fs.readFile(dataFilePath, 'utf8');
        return JSON.parse(data) as Data[];
    } catch (error) {
        console.error('Error reading data file:', error);
        return [];
    }
}

// Save data to file each time there's an INSERT
async function saveData(data: Data[], table: string): Promise<void> {
    try {
        const dataFilePath = path.join(__dirname, '..', `data/${table}.txt`);
        await fs.writeFile(dataFilePath, JSON.stringify(data, null, 2));
    } catch (error) {
        console.error('Error writing to data file:', error);
    }
}

// SQL Parser Function
function sqlParser(sql: string) {
    sql = sql.trim();

    const selectAllRegex = /^SELECT \* FROM (\w+)$/i;
    const selectWhereRegex = /^SELECT \* FROM (\w+) WHERE id = (\d+)$/i;
    const insertRegex = /^INSERT INTO (\w+) \(([^)]+)\) VALUES \(([^)]+)\)$/i;

    const selectAllMatch = sql.match(selectAllRegex);
    if (selectAllMatch) {
        const table = selectAllMatch[1];
        return {
            command: "SELECT",
            table: table,
            columns: "*",
            where: null,
        };
    }

    const selectWhereMatch = sql.match(selectWhereRegex);
    if (selectWhereMatch) {
        const table = selectWhereMatch[1];
        const id = parseInt(selectWhereMatch[2]);
        return {
            command: "SELECT",
            table: table,
            columns: "*",
            where: { id },
        };
    }

    const insertMatch = sql.match(insertRegex);
    if (insertMatch) {
        const table = insertMatch[1];
        const columns = insertMatch[2].split(",").map(col => col.trim());
        const values = insertMatch[3].split(",").map(val => val.trim());

        if (columns.length !== values.length) {
            throw new Error("Column and value count mismatch in INSERT statement");
        }

        const data: { [key: string]: any } = {};
        columns.forEach((col, index) => {
            data[col] = values[index];
        });

        return {
            command: "INSERT",
            table: table,
            data: data,
        };
    }

    throw new Error("Unsupported SQL command format");
}

// TCP Server to handle SQL-like commands
const server = net.createServer((socket) => {
    socket.on('data', async (data) => {
        const command = data.toString().trim();
        console.info('Received command:', command);

        try {
            const parsedCommand = sqlParser(command);

            if (parsedCommand.command === "SELECT") {
                // Read data from file for each SELECT operation
                const data = await loadData(parsedCommand.table);

                if (parsedCommand.where && parsedCommand.where.id !== undefined) {
                    // Handle SELECT * FROM table WHERE id = {}
                    const datum = data.find(s => s.id === parsedCommand.where.id);
                    if (datum) {
                        const response = JSON.stringify([datum]) + "\n";
                        socket.write(response);
                    } else {
                        const errorResponse = JSON.stringify({ error: 'Data not found' }) + "\n";
                        socket.write(errorResponse);
                    }
                } else {
                    // Handle SELECT * FROM table
                    const response = JSON.stringify(data) + "\n";
                    socket.write(response);
                }
            } else if (parsedCommand && parsedCommand.data && parsedCommand.command === "INSERT") {
                // Read data from file, add a new data, and save back to file
                const data = await loadData(parsedCommand.table);
                const newId = data.length > 0 ? Math.max(...data.map(s => s.id)) + 1 : 1;
                
                // Create new data and add to the array
                const newData: Data = { id: newId, name: parsedCommand.data.name, description: parsedCommand.data.description };
                data.push(newData);

                // Save updated data list back to file
                await saveData(data, parsedCommand.table);

                const response = JSON.stringify({ success: true, newData }) + "\n";
                socket.write(response);
            } else {
                // Handle unsupported command
                const errorResponse = JSON.stringify({ error: 'Unsupported command' }) + "\n";
                socket.write(errorResponse);
            }
        } catch (error) {
            console.error('Error parsing command:', error);
            const errorResponse = JSON.stringify({ error: 'Invalid SQL command' }) + "\n";
            socket.write(errorResponse);
        }
    });

    // Error handling
    socket.on('error', (err) => {
        console.error('Socket error:', err);
    });

    // Client disconnect handler
    socket.on('end', () => {
        console.info('Client disconnected');
    });
});

// Start TCP server
const PORT = 8004;
server.listen(PORT, () => {
    console.info(`TCP server listening on port ${PORT}`);
});